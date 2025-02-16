// src.rs - Translated from src.c

use std::fs;
use std::fs::File;
use std::io::{Read, Write, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::ffi::CString;
use rand::Rng;
use rand::thread_rng;
use libc;
use std::io::{stdout, stderr};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::fd::{BorrowedFd, AsRawFd};

const BLOCKSIZE: usize = 32769; // must be mod 3 = 0, should be >= 16k
const RANDOM_DEVICE: &str = "/dev/urandom"; // must not exist
const DIR_SEPARATOR: char = '/'; // '/' on unix, '\' on dos/win
const MAXINODEWIPE: usize = 4194304; // 22 bits

#[cfg(target_os = "linux")]
use libc::sync; // Linux-specific

#[cfg(not(target_os = "linux"))]
fn sync() {
    // Implement a no-op sync for non-Linux systems
    // Or use a platform-specific alternative if available
    println!("Warning: sync() is a no-op on this platform.");
}

static WRITE_MODES: [[u8; 3]; 27] = [
    [0x55, 0x55, 0x55], [0xAA, 0xAA, 0xAA], [0x92, 0x49, 0x24], [0x49, 0x24, 0x92],
    [0x24, 0x92, 0x49], [0x00, 0x00, 0x00], [0x11, 0x11, 0x11], [0x22, 0x22, 0x22],
    [0x33, 0x33, 0x33], [0x44, 0x44, 0x44], [0x55, 0x55, 0x55], [0x66, 0x66, 0x66],
    [0x77, 0x77, 0x77], [0x88, 0x88, 0x88], [0x99, 0x99, 0x99], [0xAA, 0xAA, 0xAA],
    [0xBB, 0xBB, 0xBB], [0xCC, 0xCC, 0xCC], [0xDD, 0xDD, 0xDD], [0xEE, 0xEE, 0xEE],
    [0xFF, 0xFF, 0xFF], [0x92, 0x49, 0x24], [0x49, 0x24, 0x92], [0x24, 0x92, 0x49],
    [0x6D, 0xB6, 0xDB], [0xB6, 0xDB, 0x6D], [0xDB, 0x6D, 0xB6]
];
static STD_ARRAY_FF: [u8; 3] = [0xFF, 0xFF, 0xFF];
static STD_ARRAY_00: [u8; 3] = [0x00, 0x00, 0x00];

static mut DEV_RANDOM: Option<File> = None;
static mut VERBOSE: i32 = 0;
static mut INTERNAL_SDEL_INIT: i32 = 0;

static mut SLOW: bool = true;
static mut USE_O_SYNC: bool = true;
static mut RECURSIVE: bool = false;
static mut ZERO: bool = false;
static mut BUFSIZE: usize = BLOCKSIZE;
static mut FD: i32 = -1;

fn sdel_fill_buf(pattern: &[u8; 3], bufsize: usize, buf: &mut [u8]) {
    for loop_ in 0..(bufsize / 3) {
        let where_ = loop_ * 3;
        buf[where_] = pattern[0];
        buf[where_ + 1] = pattern[1];
        buf[where_ + 2] = pattern[2];
    }
}

fn sdel_random_buf(bufsize: usize, buf: &mut [u8]) {
    unsafe {
        if DEV_RANDOM.is_none() {
            let mut rng = thread_rng();
            for loop_ in 0..bufsize {
                buf[loop_] = rng.gen::<u8>();
            }
        } else {
            DEV_RANDOM.as_mut().unwrap().read_exact(buf).expect("Failed to read from /dev/urandom");
        }
    }
}

fn sdel_random_filename(filename: &mut String) {
    let mut rng = thread_rng();
    if let Some(index) = filename.rfind(DIR_SEPARATOR) {
        for i in (index + 1..filename.len()).rev() {
            if filename.as_bytes()[i] != b'.' {
                filename.replace_range(i..i + 1, &char::from_u32(((rng.gen::<u8>() % 26) + 97) as u32).unwrap().to_string());
            }
        }
    } else {
        for i in (0..filename.len()).rev() {
            if filename.as_bytes()[i] != b'.' {
                filename.replace_range(i..i + 1, &char::from_u32(((rng.gen::<u8>() % 26) + 97) as u32).unwrap().to_string());
            }
        }
    }
}

fn sdel_init(secure_random: bool) {
    println!("sdel_init called with secure_random = {}", secure_random);
    unsafe {
        // Disable buffering for stdout and stderr (equivalent to setvbuf)
        //libc::setbuf(stdout().as_raw_fd() as *mut libc::FILE, std::ptr::null_mut());
        //libc::setbuf(stderr().as_raw_fd() as *mut libc::FILE, std::ptr::null_mut());

        if BLOCKSIZE < 16384 {
            eprintln!("Programming Warning: in-compiled blocksize is <16k !");
        }
        if BLOCKSIZE % 3 > 0 {
            eprintln!("Programming Error: in-compiled blocksize is not a multiple of 3!\n");
        }

        libc::srand(((libc::getpid() + libc::getuid() as i32 + libc::getgid() as i32) ^ libc::time(std::ptr::null_mut()) as i32) as u32);
        DEV_RANDOM = None;

        if secure_random {
            if let Ok(file) = File::open(RANDOM_DEVICE) {
                DEV_RANDOM = Some(file);
                if VERBOSE > 0 {
                    println!("Using {} for random input.", RANDOM_DEVICE);
                }
            } else {
                eprintln!("Error opening {}: {:?}", RANDOM_DEVICE, File::open(RANDOM_DEVICE).err());
            }
        } else {
            DEV_RANDOM = None;
        }

        INTERNAL_SDEL_INIT = 1;
    }
}

fn sdel_finish() {
    unsafe {
        if DEV_RANDOM.is_some() {
            DEV_RANDOM = None;
        }
        if INTERNAL_SDEL_INIT == 0 {
            eprintln!("Programming Error: sdel-lib was not initialized before calling sdel_finish().");
            return;
        }
        INTERNAL_SDEL_INIT = 0;
    }
}

/*
 * secure_overwrite function parameters:
 * mode = 0 : once overwrite with random data
 *        1 : once overwrite with 0xff, then once with random data
 *        2 : overwrite 38 times with special values
 * fd       : filedescriptor of the target to overwrite
 * start    : where to start overwriting. 0 is from the beginning
 * bufsize  : size of the buffer to use for overwriting, depends on the filesystem
 * length   : amount of data to write (file size), 0 means until an error occurs
 *
 * returns 0 on success, -1 on errors
 */
fn sdel_overwrite(mode: i32, fd: i32, start: i64, bufsize: usize, length: u64, zero: bool) -> Result<(), std::io::Error> {
    unsafe {
        if INTERNAL_SDEL_INIT == 0 {
            eprintln!("Programming Error: sdel-lib was not initialized before sdel_overwrite().");
        }

        use std::os::unix::io::{IntoRawFd, FromRawFd, AsRawFd};

        // Create a File from the file descriptor
        let file = unsafe { File::from_raw_fd(fd.as_raw_fd()) };
        let mut f = std::io::BufWriter::new(file);

        // calculate the number of writes
        let writes = if length > 0 {
            (1 + (length / bufsize as u64)) as u64
        } else {
            0
        };

        // do the first overwrite
        if start == 0 {
            f.seek(SeekFrom::Start(0))?;
        } else {
            f.seek(SeekFrom::Start(start as u64))?;
        }

        let mut buf: Vec<u8> = vec![0; bufsize];

        if mode != 0 || zero {
            if mode == 0 {
                sdel_fill_buf(&STD_ARRAY_00, bufsize, &mut buf);
            } else {
                sdel_fill_buf(&STD_ARRAY_FF, bufsize, &mut buf);
            }

            if writes > 0 {
                for _counter in 1..=writes {
                    f.write_all(&buf)?;
                }
            } else {
                loop {
                    if f.write_all(&buf).is_err() {
                        break;
                    }
                }
            }

            if VERBOSE > 0 {
                print!("*");
                stdout().flush()?;
            }
            f.flush()?;
            #[cfg(target_os = "linux")]
            sync();

            if mode == 0 {
                return Ok(());
            }
        }

        // do the rest of the overwriting stuff
        for turn in 0..=36 {
            if start == 0 {
                f.seek(SeekFrom::Start(0))?;
            } else {
                f.seek(SeekFrom::Start(start as u64))?;
            }

            if (mode < 2) && (turn > 0) {
                break;
            }

            if (turn >= 5) && (turn <= 31) {
                sdel_fill_buf(&WRITE_MODES[turn - 5], bufsize, &mut buf);

                if writes > 0 {
                    for _counter in 1..=writes {
                        f.write_all(&buf)?;
                    }
                } else {
                    loop {
                        if f.write_all(&buf).is_err() {
                            break;
                        }
                    }
                }
            } else {
                let last = zero && ((mode == 2 && turn == 36) || mode == 1);

                if last {
                    sdel_fill_buf(&STD_ARRAY_00, bufsize, &mut buf);
                }

                if writes > 0 {
                    for _counter in 1..=writes {
                        if !last {
                            sdel_random_buf(bufsize, &mut buf);
                        }
                        f.write_all(&buf)?;
                    }
                } else {
                    loop {
                        if !last {
                            sdel_random_buf(bufsize, &mut buf);
                        }
                        if f.write_all(&buf).is_err() {
                            break;
                        }
                    }
                }
            }

            f.flush()?;
            sync();

            if VERBOSE > 0 {
                print!("*");
            }
        }

        drop(f);
        sync();

        Ok(())
    }
}

/*
 * secure_unlink function parameters:
 * filename   : the file or directory to remove
 * directory  : defines if the filename poses a directory
 * truncate   : truncate file
 * slow       : do things slowly, to prevent caching
 *
 * returns 0 on success, -1 on errors.
 */
fn sdel_unlink(filename: &str, directory: bool, truncate: bool, slow: bool) -> Result<(), std::io::Error> {
    if !directory && truncate {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).truncate(true).open(filename) {
            drop(file); // Close the file
        }
    }

    let mut newname = String::from(filename);
    let mut turn = 0;
    let mut result: Result<(), std::io::Error> = Err(std::io::Error::new(std::io::ErrorKind::Other, "Initial error"));

    loop {
        sdel_random_filename(&mut newname);
        match fs::metadata(&newname) {
            Ok(_) => {
                turn += 1;
            }
            Err(_) => {
                break;
            }
        }
        if turn > 100 {
            break;
        }
    }

    if turn <= 100 {
        result = fs::rename(filename, &newname);
        if result.is_err() {
            eprintln!("Warning: Couldn't rename {} - {:?}", filename, result);
            newname = String::from(filename);
        }
    } else {
        eprintln!("Warning: Couldn't find a free filename for {}!", filename);
        newname = String::from(filename);
    }

    if directory {
        result = fs::remove_dir(&newname);
        if result.is_err() {
            eprintln!("Warning: Unable to remove directory {} - {:?}", filename, result);
            let _ = fs::rename(&newname, filename);
        } else {
            unsafe {
                if VERBOSE > 0 {
                    println!("Removed directory {} ...", filename);
                }
            }
        }
    } else {
        result = fs::remove_file(&newname);
        if result.is_err() {
            eprintln!("Warning: Unable to unlink file {} - {:?}", filename, result);
            let _ = fs::rename(&newname, filename);
        } else {
            unsafe {
                if VERBOSE > 0 {
                    println!(" Removed file {} ...", filename);
                }
            }
        }
    }

    if result.is_err() {
        return result;
    }

    Ok(())
}

fn sdel_wipe_inodes(loc: &str, _array: &mut Vec<String>) {
    unsafe {
        if VERBOSE > 0 {
            print!("Wiping inodes ...");
        }

        let mut template = String::from(loc);
        if !loc.ends_with('/') {
            template.push('/');
        }
        template.push_str("xxxxxxxx.xxx");

        let mut i = 0;
        let mut fail = 0;
        let mut array: Vec<String> = Vec::new();

        while i < MAXINODEWIPE && fail < 5 {
            sdel_random_filename(&mut template);
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&template) {
                Ok(_) => {
                    array.push(template.clone());
                    i += 1;
                }
                Err(_) => {
                    fail += 1;
                }
            }
        }
        sync();

        if fail < 5 {
            eprintln!("Warning: could not wipe all inodes!");
        }

        let mut fd = 0;
        while fd < i {
            let _ = fs::remove_file(&array[fd]);
            fd += 1;
        }
        sync();
        if VERBOSE > 0 {
            print!(" Done ... ");
        }
    }
}

fn help(prg: &str) {
    println!("{} [-dflrvz] file1 file2 etc.\n", prg);
    println!("Options:");
    println!("\t-d  ignore the two dot special files \".\" and \"..\".");
    println!("\t-f  fast (and insecure mode): no /dev/urandom, no synchronize mode.");
    println!("\t-l  lessens the security (use twice for total insecure mode).");
    println!("\t-r  recursive mode, deletes all subdirectories.");
    println!("\t-v  is verbose mode.");
    println!("\t-z  last wipe writes zeros instead of random data.");
    println!("\nDoes a secure overwrite/rename/delete of the target file(s).");
    println!("Default is secure mode (38 writes).");
    std::process::exit(1);
}

fn smash_it(filename: &str, mode: i32) -> Result<(), String> {
    let mut filestat = match fs::metadata(filename) {
        Ok(metadata) => metadata,
        Err(_) => return Err("Error getting file metadata".to_string()),
    };

    if filestat.is_file() && filestat.nlink() > 1 {
        return Err(format!(
            "Error: File {} - file is hardlinked {} time(s), skipping!",
            filename,
            filestat.nlink() - 1
        ));
    }

    unsafe {
        // if the blocksize on the filesystem is bigger than the on compiled with, enlarge!
        if filestat.blksize() > BUFSIZE as u64 {
            if filestat.blksize() > 65532 {
                BUFSIZE = 65535;
            } else {
                BUFSIZE = (((filestat.blksize() / 3) + 1) * 3) as usize;
            }
        }

        // handle the recursive mode
        if RECURSIVE {
            if filestat.is_dir() {
                println!("DIRECTORY (going recursive now)");
                let current_dir = std::env::current_dir().unwrap();

                // a won race will chmod a file to 0700 if the user is owner/root
                // I'll think about a secure solution to this, however, I think
                // there isn't one - anyone with an idea?
                if std::env::set_current_dir(filename).is_err() {
                    let _ = fs::set_permissions(filename, fs::Permissions::from_mode(0o700)); // ignore permission errors
                    if std::env::set_current_dir(filename).is_err() {
                        return Err(format!("Can't chdir() to {}, hence I can't wipe it.", filename));
                    }
                }

                let controlstat = fs::metadata(".").unwrap();
                let cwd_stat = fs::metadata("..").unwrap();
                if filestat.dev() != controlstat.dev() || filestat.ino() != controlstat.ino() {
                    return Err(format!("Race found! (directory {} became a link)", filename));
                } else {
                    let dir = match fs::read_dir(".") {
                        Ok(dir) => dir,
                        Err(_) => {
                            let _ = fs::set_permissions(".", fs::Permissions::from_mode(0o700)); // ignore permission errors
                            match fs::read_dir(".") {
                                Ok(dir) => dir,
                                Err(e) => return Err(format!("Couldn't open dir: {}", e)),
                            }
                        }
                    };

                    for entry in dir {
                        let dir_entry = entry.unwrap();
                        if dir_entry.file_name() != "." && dir_entry.file_name() != ".." {
                            print!("Wiping {} ", dir_entry.file_name().to_str().unwrap());
                            match smash_it(dir_entry.file_name().to_str().unwrap(), mode) {
                                Ok(_) => {
                                    println!(" Done");
                                }
                                Err(e) => {
                                    eprintln!("Couldn't delete {}. {}", dir_entry.file_name().to_str().unwrap(), e);
                                }
                            }
                        }
                    }
                }
                if std::env::set_current_dir(current_dir).is_err() {
                    return Err("Error: Can't chdir to previous directory (aborting)".to_string());
                }
                return sdel_unlink(filename, true, false, SLOW).map_err(|e| e.to_string());
            }
        }

        if filestat.is_file() {
            // open the file for writing in sync. mode
            let open_result = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(if SLOW { libc::O_SYNC } else { 0 })
                .open(filename);

            let mut fd = match open_result {
                Ok(file) => file,
                Err(_) => {
                    // here again this has a race problem ... hmmm
                    // make it writable for us if possible
                    let _ = fs::set_permissions(filename, fs::Permissions::from_mode(0o600)); // ignore errors
                    match std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .custom_flags(if SLOW { libc::O_SYNC } else { 0 })
                        .open(filename) {
                        Ok(file) => file,
                        Err(e) => return Err(format!("Couldn't open file: {}", e)),
                    }
                }
            };

            filestat = match fs::metadata(filename) {
                Ok(metadata) => metadata,
                Err(_) => return Err("Error getting file metadata".to_string()),
            };

            let controlstat = fd.metadata().unwrap();
            if filestat.dev() != controlstat.dev() || filestat.ino() != controlstat.ino() || !controlstat.is_file() {
                return Err("File raced!".to_string());
            }

            let file_size = filestat.len();
            sdel_overwrite(mode, FD, 0, BUFSIZE, file_size, ZERO).map_err(|e| e.to_string())?;
            return sdel_unlink(filename, false, true, SLOW).map_err(|e| e.to_string());
        } else {
            if filestat.is_dir() {
                return Err(format!(
                    "Warning: {} is a directory. I will not remove it, because the -r option is missing!",
                    filename
                ));
            } else {
                eprintln!("Warning: {} is not a regular file, rename/unlink only!", filename);
                return sdel_unlink(filename, false, false, SLOW).map_err(|e| e.to_string());
            }
        }
    }
}


fn main() -> Result<(), String> {
    println!("main called");
    let mut errors = 0;
    let mut dot = false;
    let mut secure = 2; // Standard is now SECURE mode (38 overwrites) [since v2.0]
    let mut args: Vec<String> = std::env::args().collect();
    let prg = args[0].clone();

    if args.len() < 2 || args[1].starts_with("-h") || args[1].starts_with("--h") {
        help(&prg);
    }

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "-D" => dot = true,
            "-f" | "-F" => {
                unsafe { SLOW = false; }
                println!("-f flag set, SLOW = {}", unsafe { SLOW });
            }
            "-l" | "-L" => {
                if secure > 0 {
                    secure -= 1;
                }
            }
            "-r" | "-R" => unsafe { RECURSIVE = true; },
            "-s" | "-S" => secure += 1,
            "-v" | "-V" => unsafe { VERBOSE += 1; },
            "-z" | "-Z" => unsafe { ZERO = true; },
            _ => break,
        }
        i += 1;
    }

    if i == args.len() {
        help(&prg);
    }

    unsafe {
            extern "C" fn cleanup(signo: i32) {
                eprintln!("Terminated by signal. Clean exit.");
                unsafe {
                    if FD >= 0 {
                        let _ = libc::close(FD);
                    }
                }
                unsafe { sync(); }
                std::process::exit(1);
            }
            libc::signal(libc::SIGINT, unsafe { std::mem::transmute(cleanup as unsafe extern "C" fn(libc::c_int) -> ()) });
            libc::signal(libc::SIGTERM, unsafe { std::mem::transmute(cleanup as unsafe extern "C" fn(libc::c_int) -> ()) });
            libc::signal(libc::SIGHUP, unsafe { std::mem::transmute(cleanup as unsafe extern "C" fn(libc::c_int) -> ()) });
    }

    unsafe {
        if VERBOSE > 0 {
            let type_ = if ZERO { "zero" } else { "random" };
            match secure {
                0 => { println!("Wipe mode is insecure (one pass [{}])", type_); }
                1 => { println!("Wipe mode is insecure (two passes [0xff/{}])", type_); }
                _ => { println!("Wipe mode is secure (38 special passes)"); }
            }
        }
    }

    println!("SLOW = {}", unsafe { SLOW });
    sdel_init(unsafe { SLOW });

    // Removed RLIMIT_INFINITY and RLIMIT_FSIZE related code

    while i < args.len() {
        let rmfile = args[i].clone();
        i += 1;
        if rmfile == "/" {
            eprintln!("Not going to let you delete the ROOT directory.");
            return Err("Deleting root directory is not allowed".to_string());
        }
        if dot {
            if rmfile == "." || rmfile == ".." {
                continue;
            }
        }
        unsafe {
            if VERBOSE > 0 {
                print!("Wiping {} ", rmfile);
            }
        }
        match smash_it(&rmfile, secure) {
            Ok(_) => unsafe {
                if VERBOSE > 0 {
                    println!(" Done");
                }
            },
            Err(e) => {
                eprintln!("Error: File {} - {}", rmfile, e);
                errors += 1;
            }
        }
    }

    sdel_finish();

    if errors > 0 {
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

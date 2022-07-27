#include <stdio.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <strings.h>
#include <string.h>
#include <unistd.h>
#include <stdlib.h>
#include <dirent.h>
#include <signal.h>
#include <time.h>
#include <sys/time.h>
#include <sys/resource.h>

#define BLOCKSIZE 32769              /* must be mod 3 = 0, should be >= 16k */
#define RANDOM_DEVICE "/dev/urandom" /* must not exist */
#define DIR_SEPARATOR '/'            /* '/' on unix, '\' on dos/win */
#define FLUSH sync()                 /* system call to flush the disk */
#define MAXINODEWIPE 4194304         /* 22 bits */

#ifndef O_SYNC
#ifdef O_FSYNC
#define O_SYNC O_FSYNC
#else
#define O_SYNC 0
#endif
#endif

#ifndef O_LARGEFILE
#define O_LARGEFILE 0
#endif

char* prg;

unsigned char write_modes[27][3] = {{"\x55\x55\x55"}, {"\xaa\xaa\xaa"}, {"\x92\x49\x24"}, {"\x49\x24\x92"},
                                    {"\x24\x92\x49"}, {"\x00\x00\x00"}, {"\x11\x11\x11"}, {"\x22\x22\x22"},
                                    {"\x33\x33\x33"}, {"\x44\x44\x44"}, {"\x55\x55\x55"}, {"\x66\x66\x66"},
                                    {"\x77\x77\x77"}, {"\x88\x88\x88"}, {"\x99\x99\x99"}, {"\xaa\xaa\xaa"},
                                    {"\xbb\xbb\xbb"}, {"\xcc\xcc\xcc"}, {"\xdd\xdd\xdd"}, {"\xee\xee\xee"},
                                    {"\xff\xff\xff"}, {"\x92\x49\x24"}, {"\x49\x24\x92"}, {"\x24\x92\x49"},
                                    {"\x6d\xb6\xdb"}, {"\xb6\xdb\x6d"}, {"\xdb\x6d\xb6"}};
unsigned char std_array_ff[3]    = "\xff\xff\xff";
unsigned char std_array_00[3]    = "\x00\x00\x00";

FILE* devrandom            = NULL;
int   verbose              = 0;
int   __internal_sdel_init = 0;

int           slow      = O_SYNC;
int           recursive = 0;
int           zero      = 0;
unsigned long bufsize   = BLOCKSIZE;
int           fd;

void __sdel_fill_buf(char pattern[3], unsigned long bufsize, char* buf)
{
    int loop;
    int where;

    for (loop = 0; loop < (bufsize / 3); loop++)
    {
        where  = loop * 3;
        *buf++ = pattern[0];
        *buf++ = pattern[1];
        *buf++ = pattern[2];
    }
}

void __sdel_random_buf(unsigned long bufsize, char* buf)
{
    int loop;

    if (devrandom == NULL)
        for (loop = 0; loop < bufsize; loop++)
            *buf++ = (unsigned char)(256.0 * rand() / (RAND_MAX + 1.0));
    else
        fread(buf, bufsize, 1, devrandom);
}

void __sdel_random_filename(char* filename)
{
    int i;
    for (i = strlen(filename) - 1; (filename[i] != DIR_SEPARATOR) && (i >= 0); i--)
        if (filename[i] != '.') /* keep dots in the filename */
            filename[i] = 97 + (int)((int)((256.0 * rand()) / (RAND_MAX + 1.0)) % 26);
}

void sdel_init(int secure_random)
{
    (void)setvbuf(stdout, NULL, _IONBF, 0);
    (void)setvbuf(stderr, NULL, _IONBF, 0);

    if (BLOCKSIZE < 16384)
        fprintf(stderr, "Programming Warning: in-compiled blocksize is <16k !\n");
    if (BLOCKSIZE % 3 > 0)
        fprintf(stderr, "Programming Error: in-compiled blocksize is not a multiple of 3!\n");

    srand((getpid() + getuid() + getgid()) ^ time(0));
    devrandom = NULL;
#ifdef RANDOM_DEVICE
    if (secure_random)
    {
        if ((devrandom = fopen(RANDOM_DEVICE, "r")) != NULL)
            if (verbose)
                printf("Using %s for random input.\n", RANDOM_DEVICE);
    }
#endif

    __internal_sdel_init = 1;
}

void sdel_finish()
{
    if (devrandom != NULL)
    {
        fclose(devrandom);
        devrandom = NULL;
    }
    if (!__internal_sdel_init)
    {
        fprintf(stderr, "Programming Error: sdel-lib was not initialized before calling sdel_finish().\n");
        return;
    }
    __internal_sdel_init = 0;
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
int sdel_overwrite(int mode, int fd, long start, unsigned long bufsize, unsigned long length, int zero)
{
    unsigned long writes;
    unsigned long counter;
    int           turn;
    int           last = 0;
    char          buf[65535];
    FILE*         f;

    if (!__internal_sdel_init)
        fprintf(stderr, "Programming Error: sdel-lib was not initialized before sdel_overwrite().\n");

    if ((f = fdopen(fd, "r+b")) == NULL)
        return -1;

    /* calculate the number of writes */
    if (length > 0)
        writes = (1 + (length / bufsize));
    else
        writes = 0;

    /* do the first overwrite */
    if (start == 0)
        rewind(f);
    else if (fseek(f, start, SEEK_SET) != 0)
        return -1;
    if (mode != 0 || zero)
    {
        if (mode == 0)
            __sdel_fill_buf((char*)std_array_00, bufsize, buf);
        else
            __sdel_fill_buf((char*)std_array_ff, bufsize, buf);
        if (writes > 0)
            for (counter = 1; counter <= writes; counter++)
                fwrite(&buf, 1, bufsize, f); // dont care for errors
        else
            do
            {
            } while (fwrite(&buf, 1, bufsize, f) == bufsize);
        if (verbose)
            printf("*");
        fflush(f);
        if (fsync(fd) < 0)
            FLUSH;
        if (mode == 0)
            return 0;
    }

    /* do the rest of the overwriting stuff */
    for (turn = 0; turn <= 36; turn++)
    {
        if (start == 0)
            rewind(f);
        else if (fseek(f, start, SEEK_SET) != 0)
            return -1;
        if ((mode < 2) && (turn > 0))
            break;
        if ((turn >= 5) && (turn <= 31))
        {
            __sdel_fill_buf((char*)write_modes[turn - 5], bufsize, buf);
            if (writes > 0)
                for (counter = 1; counter <= writes; counter++)
                    fwrite(&buf, 1, bufsize, f); // dont care for errors
            else
                do
                {
                } while (fwrite(&buf, 1, bufsize, f) == bufsize);
        }
        else
        {
            if (zero && ((mode == 2 && turn == 36) || mode == 1))
            {
                last = 1;
                __sdel_fill_buf((char*)std_array_00, bufsize, buf);
            }
            if (writes > 0)
            {
                for (counter = 1; counter <= writes; counter++)
                {
                    if (!last)
                        __sdel_random_buf(bufsize, buf);
                    fwrite(&buf, 1, bufsize, f); // dont care for errors
                }
            }
            else
            {
                do
                {
                    if (!last)
                        __sdel_random_buf(bufsize, buf);
                } while (fwrite(&buf, 1, bufsize, f) == bufsize); // dont care for errors
            }
        }
        fflush(f);
        if (fsync(fd) < 0)
            FLUSH;
        if (verbose)
            printf("*");
    }

    (void)fclose(f);
    /* Hard Flush -> Force cached data to be written to disk */
    FLUSH;

    return 0;
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
int sdel_unlink(char* filename, int directory, int truncate, int slow)
{
    int         fd;
    int         turn = 0;
    int         result;
    char        newname[strlen(filename) + 1];
    struct stat filestat;

    /* open + truncating the file, so an attacker doesn't know the diskblocks */
    if (!directory && truncate)
        if ((fd = open(filename, O_WRONLY | O_TRUNC | slow)) >= 0)
            close(fd);

    /* Generate random unique name, renaming and deleting of the file */
    strcpy(newname, filename); // not a buffer overflow as it has got the exact length

    do
    {
        __sdel_random_filename(newname);
        if ((result = lstat(newname, &filestat)) >= 0)
            turn++;
    } while ((result >= 0) && (turn <= 100));

    if (turn <= 100)
    {
        result = rename(filename, newname);
        if (result != 0)
        {
            fprintf(stderr, "Warning: Couldn't rename %s - ", filename);
            perror("");
            strcpy(newname, filename);
        }
    }
    else
    {
        fprintf(stderr, "Warning: Couldn't find a free filename for %s!\n", filename);
        strcpy(newname, filename);
    }

    if (directory)
    {
        result = rmdir(newname);
        if (result)
        {
            printf("Warning: Unable to remove directory %s - ", filename);
            perror("");
            (void)rename(newname, filename);
        }
        else if (verbose)
            printf("Removed directory %s ...", filename);
    }
    else
    {
        result = unlink(newname);
        if (result)
        {
            printf("Warning: Unable to unlink file %s - ", filename);
            perror("");
            (void)rename(newname, filename);
        }
        else if (verbose)
            printf(" Removed file %s ...", filename);
    }

    if (result != 0)
        return -1;

    return 0;
}

void sdel_wipe_inodes(char* loc, char** array)
{
    char* template = malloc(strlen(loc) + 16);
    int i          = 0;
    int fail       = 0;
    int fd;

    if (verbose)
        printf("Wiping inodes ...");

    array = malloc(MAXINODEWIPE * sizeof(template));
    strcpy(template, loc);
    if (loc[strlen(loc) - 1] != '/')
        strcat(template, "/");
    strcat(template, "xxxxxxxx.xxx");

    while (i < MAXINODEWIPE && fail < 5)
    {
        __sdel_random_filename(template);
        if (open(template, O_CREAT | O_EXCL | O_WRONLY, 0600) < 0)
            fail++;
        else
        {
            array[i] = malloc(strlen(template));
            strcpy(array[i], template);
            i++;
        }
    }
    FLUSH;

    if (fail < 5)
    {
        fprintf(stderr, "Warning: could not wipe all inodes!\n");
    }

    array[i] = NULL;
    fd       = 0;
    while (fd < i)
    {
        unlink(array[fd]);
        free(array[fd]);
        fd++;
    }
    free(array);
    array = NULL;
    FLUSH;
    if (verbose)
        printf(" Done ... ");
}

void help()
{
    printf("%s [-dflrvz] file1 file2 etc.\n\n", prg);
    printf("Options:\n");
    printf("\t-d  ignore the two dot special files \".\" and \"..\".\n");
    printf("\t-f  fast (and insecure mode): no /dev/urandom, no synchronize mode.\n");
    printf("\t-l  lessens the security (use twice for total insecure mode).\n");
    printf("\t-r  recursive mode, deletes all subdirectories.\n");
    printf("\t-v  is verbose mode.\n");
    printf("\t-z  last wipe writes zeros instead of random data.\n");
    printf("\nDoes a secure overwrite/rename/delete of the target file(s).\n");
    printf("Default is secure mode (38 writes).\n");
    exit(1);
}

int smash_it(char* filename, int mode)
{
    struct stat filestat;
    struct stat controlstat;
    int         i_am_a_directory = 0;

    /* get the file stats */
    if (lstat(filename, &filestat))
        return 1;

    if (S_ISREG(filestat.st_mode) && filestat.st_nlink > 1)
    {
        fprintf(stderr,
                "Error: File %s - file is hardlinked %lu time(s), skipping!\n",
                filename,
                (unsigned long)filestat.st_nlink - 1);
        return -1;
    }

    /* if the blocksize on the filesystem is bigger than the on compiled with, enlarge! */
    if (filestat.st_blksize > bufsize)
    {
        if (filestat.st_blksize > 65532)
        {
            bufsize = 65535;
        }
        else
        {
            bufsize = (((filestat.st_blksize / 3) + 1) * 3);
        }
    }

    /* handle the recursive mode */
    if (recursive)
        if (S_ISDIR(filestat.st_mode))
        {
            DIR*           dir;
            struct dirent* dir_entry;
            struct stat    cwd_stat;
            char           current_dir[4097];
            int            res;
            int            chdir_success = 1;

            if (verbose)
                printf("DIRECTORY (going recursive now)\n");
            getcwd(current_dir, 4096);
            current_dir[4096] = '\0';

            /* a won race will chmod a file to 0700 if the user is owner/root
               I'll think about a secure solution to this, however, I think
               there isn't one - anyone with an idea? */
            if (chdir(filename))
            {
                (void)chmod(filename, 0700); /* ignore permission errors */
                if (chdir(filename))
                {
                    fprintf(stderr, "Can't chdir() to %s, hence I can't wipe it.\n", filename);
                    chdir_success = 0;
                }
            }

            if (chdir_success)
            {
                lstat(".", &controlstat);
                lstat("..", &cwd_stat);
                if ((filestat.st_dev != controlstat.st_dev) || (filestat.st_ino != controlstat.st_ino))
                {
                    fprintf(stderr, "Race found! (directory %s became a link)\n", filename);
                }
                else
                {
                    if ((dir = opendir(".")) != NULL)
                    {
                        (void)chmod(".", 0700); /* ignore permission errors */
                        dir = opendir(".");
                    }
                    if (dir != NULL)
                    {
                        while ((dir_entry = readdir(dir)) != NULL)
                            if (strcmp(dir_entry->d_name, ".") && strcmp(dir_entry->d_name, ".."))
                            {
                                if (verbose)
                                    printf("Wiping %s ", dir_entry->d_name);
                                if ((res = smash_it(dir_entry->d_name, mode)) > 0)
                                {
                                    if (res == 3)
                                        fprintf(stderr,
                                                "File %s was raced, hence I won't wipe it.\n",
                                                dir_entry->d_name);
                                    else
                                    {
                                        fprintf(stderr, "Couldn't delete %s. ", dir_entry->d_name);
                                        perror("");
                                    }
                                }
                                else if (verbose)
                                    printf(" Done\n");
                            }
                        closedir(dir);
                    }
                }
                if (chdir(current_dir) != 0)
                {
                    fprintf(stderr, "Error: Can't chdir to %s (aborting) - ", current_dir);
                    perror("");
                    exit(1);
                }

                i_am_a_directory = 1;
            }
        }
    /* end of recursive function */

    if (S_ISREG(filestat.st_mode))
    {
        /* open the file for writing in sync. mode */
        if ((fd = open(filename, O_RDWR | O_LARGEFILE | slow)) < 0)
        {
            /* here again this has a race problem ... hmmm */
            /* make it writable for us if possible */
            (void)chmod(filename, 0600); /* ignore errors */
            if ((fd = open(filename, O_RDWR | O_LARGEFILE | slow)) < 0)
                return 1;
        }

        fstat(fd, &controlstat);
        if ((filestat.st_dev != controlstat.st_dev) || (filestat.st_ino != controlstat.st_ino) ||
            (!S_ISREG(controlstat.st_mode)))
        {
            close(fd);
            return 3;
        }

        if (sdel_overwrite(mode, fd, 0, bufsize, filestat.st_size > 0 ? filestat.st_size : 1, zero) == 0)
            return sdel_unlink(filename, 0, 1, slow);
    } /* end IS_REG() */
    else
    {
        if (S_ISDIR(filestat.st_mode))
        {
            if (i_am_a_directory == 0)
            {
                fprintf(stderr,
                        "Warning: %s is a directory. I will not remove it, because the -r option is missing!\n",
                        filename);
                return 0;
            }
            else
                return sdel_unlink(filename, 1, 0, slow);
        }
        else if (!S_ISDIR(filestat.st_mode))
        {
            fprintf(stderr, "Warning: %s is not a regular file, rename/unlink only!", filename);
            if (!verbose)
                printf("\n");
            return sdel_unlink(filename, 0, 0, slow);
        }
    }

    return 99; // not reached
}

void cleanup(int signo)
{
    fprintf(stderr, "Terminated by signal. Clean exit.\n");
    if (fd >= 0)
        close(fd);
    FLUSH;
    exit(1);
}

int main(int argc, char* argv[])
{
    int           errors = 0;
    int           dot    = 0;
    int           result;
    int           secure = 2; /* Standard is now SECURE mode (38 overwrites) [since v2.0] */
    int           loop;
    struct rlimit rlim;

    prg = argv[0];
    if (argc < 2 || strncmp(argv[1], "-h", 2) == 0 || strncmp(argv[1], "--h", 3) == 0)
        help();

    while (1)
    {
        result = getopt(argc, argv, "DdFfLlRrSsVvZz");
        if (result < 0)
            break;
        switch (result)
        {
        case 'd':
        case 'D':
            dot = 1;
            break;
        case 'F':
        case 'f':
            slow = 0;
            break;
        case 'L':
        case 'l':
            if (secure)
                secure--;
            break;
        case 'R':
        case 'r':
            recursive++;
            break;
        case 'S':
        case 's':
            secure++;
            break;
        case 'V':
        case 'v':
            verbose++;
            break;
        case 'Z':
        case 'z':
            zero++;
            break;
        default:
            help();
        }
    }
    loop = optind;
    if (loop == argc)
        help();

    signal(SIGINT, cleanup);
    signal(SIGTERM, cleanup);
    signal(SIGHUP, cleanup);

    sdel_init(slow);

    if (verbose)
    {
        char type[15] = "random";
        if (zero)
            strcpy(type, "zero");
        switch (secure)
        {
        case 0:
            printf("Wipe mode is insecure (one pass [%s])\n", type);
            break;
        case 1:
            printf("Wipe mode is insecure (two passes [0xff/%s])\n", type);
            break;
        default:
            printf("Wipe mode is secure (38 special passes)\n");
        }
    }

#ifdef RLIM_INFINITY
#ifdef RLIMIT_FSIZE
    rlim.rlim_cur = RLIM_INFINITY;
    rlim.rlim_max = RLIM_INFINITY;
    if (setrlimit(RLIMIT_FSIZE, &rlim) != 0)
        fprintf(stderr, "Warning: Could not reset ulimit for filesize.\n");
#else
    fprintf(stderr, "Warning: Not compiled with support for resetting ulimit filesize.\n");
#endif
#endif

    while (loop < argc)
    {
        char rmfile[strlen(argv[loop]) + 1];
        strcpy(rmfile, argv[loop]);
        loop++;
        if (strcmp("/", rmfile) == 0)
        {
            fprintf(stderr, "Not going to let you delete the ROOT directory.");
            return 1;
        }
        if (dot)
            if ((strcmp(".", rmfile) == 0) || (strcmp("..", rmfile) == 0))
                continue;
        if (verbose)
            printf("Wiping %s ", rmfile);
        result = (int)smash_it(rmfile, secure);
        switch (result)
        {
        case 0:
            if (verbose)
                printf(" Done\n");
            break;
        case 1:
            fprintf(stderr, "Error: File %s - ", rmfile);
            perror("");
            break;
        case -1:
            break;
        case 3:
            fprintf(stderr, "File %s was raced, hence I won't wipe it!\n", rmfile);
            break;
        default:
            fprintf(stderr, "Unknown error\n");
        }
        if (result)
            errors++;
    }

    sdel_finish();

    if (errors)
        return 1;
    else
        return 0;
}

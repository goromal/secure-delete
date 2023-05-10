# secure-delete

![example workflow](https://github.com/goromal/secure-delete/actions/workflows/test.yml/badge.svg)

Condensed version of the secure remove utility (https://github.com/cryptisk-grs/thc-secure-delete).

## Building

The program is written in pure c, with only standard library and system dependencies. Should compile on any Unix-like system.

```bash
gcc src.c
```

## Usage

```bash
secure-delete [-dflrvz] file1 file2 etc.

Options:
        -d  ignore the two dot special files "." and "..".
        -f  fast (and insecure mode): no /dev/urandom, no synchronize mode.
        -l  lessens the security (use twice for total insecure mode).
        -r  recursive mode, deletes all subdirectories.
        -v  is verbose mode.
        -z  last wipe writes zeros instead of random data.

Does a secure overwrite/rename/delete of the target file(s).
Default is secure mode (38 writes).
```


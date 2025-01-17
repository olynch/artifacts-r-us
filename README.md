# Artifacts R Us

This is an extremely simple server for software artifacts (that is, compiled releases of software). It's probably less secure and less featureful than [JFrog Artifactory](https://jfrog.com/artifactory/), but it's more fun! Also I couldn't figure out how to set up jfrog.

## Storage

Storage is done in the filesystem. On startup, the commandline argument `--state-dir` is used to specify a directory where the server should save files. The files associated with a software release are stored in `<state-dir>/<project>/versions/<version>/files/*`.

The "administration interface" for Artifacts R Us is simply the filesystem: adding new projects and managing their permissions can be simply done over SSH.

## Authorization

Authorization is done with HTTP bearer tokens, specifically with the `Authorization: Bearer XXXX` header.

Example usage with `curl`:

```
curl -H 'Authorization: Bearer XXXX'
```

Authorization is managed per-project by having `<project>/readers.txt` and `<project>/writers.txt`. This is a newline-separated list of bearer tokens which are permitted to read and write to the project, respectively, which is managed by administrator.

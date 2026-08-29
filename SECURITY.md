# Security Policy

## Supported versions

Only the latest published release of gdmerge is supported with security fixes.
Please upgrade before reporting an issue if you are not on the latest release.

## Reporting a vulnerability

gdmerge parses `.tscn`/`.tres` scene and resource files, which are often untrusted
input (they can come from other contributors, forks, or the internet). The
main risks are a malformed or adversarial file causing a crash, excessive
resource usage, or an incorrect merge result that silently corrupts a scene.

If you find an issue like this, please **do not open a public issue**. Instead,
use GitHub's private vulnerability reporting for this repository:

https://github.com/hyprtuna/gdmerge/security/advisories/new

Include a description of the issue and, if possible, a sample `.tscn`/`.tres`
file that reproduces it. We'll acknowledge reports as soon as we can and work
with you on a fix and disclosure timeline.

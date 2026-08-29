# Security Policy

## Supported versions

Only the latest published release of gdmerge is supported with security fixes.
Please upgrade before reporting an issue if you are not on the latest release.

## Reporting a vulnerability

gdmerge parses `.tscn`/`.tres` scene and resource files, which are often untrusted
input (they can come from other contributors, forks, or the internet). The
main risks are a malformed or adversarial file causing a crash, excessive
resource usage, or an incorrect merge result that silently corrupts a scene.

If you find an issue like this, please **do not describe it in a public issue**.

GitHub's private vulnerability reporting is not enabled on this repository yet.
Until it is, open a security contact request, an issue that carries no details:

https://github.com/hyprtuna/gdmerge/issues/new?template=security_contact.yml

The maintainer will reply on it with a private channel. Send the description
there and, if possible, a sample `.tscn`/`.tres` file that reproduces the
issue. Reports are acknowledged as soon as possible, and the fix and the
disclosure timeline are worked out with you.

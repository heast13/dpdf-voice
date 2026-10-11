# Security

dpdf-voice is a VST2 plugin that Equalizer APO loads into the Windows audio
service (`audiodg.exe`), so it runs inside that process. It writes
only its log file (`dpdf-voice.log` in the audio service's temp folder), makes
no network connections and starts no other programs. The models and libraries
it is built with are checked against their SHA256 at build time (`scripts/fetch-assets.ps1`).

Please report anything that could let other programs or users abuse it
privately through GitHub's
[security advisories](https://github.com/heast13/dpdf-voice/security/advisories/new),
not as a public issue.

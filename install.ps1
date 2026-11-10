# devpage's installer for Windows, served by devpage.ai as
# https://devpage.ai/install.ps1: it runs the install script the latest release
# carries, which downloads the binary for this system (docs/installing.md).
$ErrorActionPreference = 'Stop'
Invoke-RestMethod https://github.com/andrei512/devpage/releases/latest/download/devpage-installer.ps1 | Invoke-Expression

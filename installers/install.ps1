# Volt WebUI Installer for Windows
# Self-contained PowerShell installer. Run as Administrator for system-wide install,
# or as a regular user for per-user install (default).
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -SystemInstall
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall
#
# What it does:
#   1. Copies webui.exe + assets to $env:LOCALAPPDATA\Volt\ (or %ProgramFiles%\Volt for -SystemInstall)
#   2. Creates Start Menu shortcuts
#   3. Creates a Desktop shortcut
#   4. Optionally adds the install dir to user PATH
#   5. Writes an uninstaller to the same dir
#   6. Records the install in the Windows registry for Add/Remove Programs

[CmdletBinding()]
param(
    [switch]$SystemInstall,
    [switch]$Uninstall,
    [switch]$NoDesktopShortcut,
    [switch]$NoPath,
    [string]$SourceDir,
    [string]$BinaryName = "webui.exe"
)

$ErrorActionPreference = "Stop"

# ----- Config -----
$ProductName = "Volt"
$DisplayName = "Volt WebUI"
$Publisher = "Volt Project"
$AppVersion = "0.7.1"
$InstallGuid = "{8B3E5C8A-2D1A-4B7E-9F3C-1A5B7C9D2E4F}"
$StartMenuFolder = "Volt"

# ----- Resolve SourceDir -----
if (-not $SourceDir) {
    if ($MyInvocation.MyCommand.Path) {
        $SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    } elseif ($PSScriptRoot) {
        $SourceDir = $PSScriptRoot
    } else {
        $SourceDir = (Get-Location).Path
    }
}

# Determine install location
if ($SystemInstall) {
    $InstallDir = Join-Path $env:ProgramFiles "Volt"
    $ShortcutScope = "AllUsers"
    $RegistryUninstallKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$ProductName"
    $NeedsAdmin = $true
} else {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Volt"
    $ShortcutScope = "CurrentUser"
    $RegistryUninstallKey = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$ProductName"
    $NeedsAdmin = $false
}

# ----- Helpers -----
function Write-Status($msg) {
    Write-Host "[Volt Installer] " -ForegroundColor Cyan -NoNewline
    Write-Host $msg
}

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $pr = New-Object Security.Principal.WindowsPrincipal($id)
    return $pr.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Create-Shortcut($target, $shortcutPath, $iconPath, $description) {
    $wsh = New-Object -ComObject WScript.Shell
    $sc = $wsh.CreateShortcut($shortcutPath)
    $sc.TargetPath = $target
    $sc.WorkingDirectory = Split-Path -Parent $target
    $sc.IconLocation = if ($iconPath) { $iconPath } else { $target }
    $sc.Description = $description
    $sc.Save()
    [System.Runtime.Interopservices.Marshal]::ReleaseComObject($wsh) | Out-Null
}

# ----- Uninstall -----
if ($Uninstall) {
    Write-Status "Uninstalling $DisplayName from $InstallDir..."
    if (Test-Path $InstallDir) {
        Remove-Item -Recurse -Force $InstallDir
    }
    $startMenu = [Environment]::GetFolderPath("StartMenu")
    $smPath = Join-Path $startMenu "Programs\$StartMenuFolder"
    if (Test-Path $smPath) {
        Remove-Item -Recurse -Force $smPath
    }
    $desktop = [Environment]::GetFolderPath("Desktop")
    $deskLnk = Join-Path $desktop "$DisplayName.lnk"
    if (Test-Path $deskLnk) {
        Remove-Item -Force $deskLnk
    }
    if (Test-Path $RegistryUninstallKey) {
        Remove-Item -Path $RegistryUninstallKey -Recurse -Force
    }
    Write-Status "Uninstall complete."
    exit 0
}

# ----- Pre-flight -----
if ($NeedsAdmin -and -not (Test-Admin)) {
    Write-Status "System-wide install requires Administrator. Re-run as Administrator or omit -SystemInstall." -ForegroundColor Red
    Write-Status "Defaulting to per-user install at $InstallDir"
    $InstallDir = Join-Path $env:LOCALAPPDATA "Volt"
    $ShortcutScope = "CurrentUser"
    $RegistryUninstallKey = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$ProductName"
}

$sourceBinary = Join-Path $SourceDir $BinaryName
if (-not (Test-Path $sourceBinary)) {
    Write-Status "Source binary not found: $sourceBinary" -ForegroundColor Red
    Write-Status "Run this installer from the directory containing $BinaryName (or pass -SourceDir <path>)."
    exit 1
}

# ----- Install -----
Write-Status "Installing $DisplayName v$AppVersion to $InstallDir"

# 1. Create install directory + copy binary
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}
Copy-Item -Path $sourceBinary -Destination $InstallDir -Force
Write-Status "  Copied $BinaryName"

# 2. Copy README + LICENSE if present
foreach ($doc in @("README.md", "LICENSE", "CHANGELOG.md", "AGENTS.md")) {
    $src = Join-Path $SourceDir $doc
    if (Test-Path $src) {
        Copy-Item -Path $src -Destination $InstallDir -Force
    }
}

# 3. Copy .env.example (template, never the user's real .env)
$envExample = Join-Path $SourceDir ".env.example"
if (Test-Path $envExample) {
    Copy-Item -Path $envExample -Destination $InstallDir -Force
}

# 4. Create Start Menu shortcuts
$startMenu = [Environment]::GetFolderPath("StartMenu")
$smPath = Join-Path $startMenu "Programs\$StartMenuFolder"
if (-not (Test-Path $smPath)) {
    New-Item -ItemType Directory -Path $smPath -Force | Out-Null
}
$appExe = Join-Path $InstallDir $BinaryName
$startMenuShortcut = Join-Path $smPath "$DisplayName.lnk"
$startMenuUninstall = Join-Path $smPath "Uninstall $DisplayName.lnk"
Create-Shortcut $appExe $startMenuShortcut $appExe "Volt WebUI - AI agent with full Postgres-backed harness"
Write-Status "  Created Start Menu shortcut: $startMenuShortcut"

# 5. Create Desktop shortcut (unless suppressed)
if (-not $NoDesktopShortcut) {
    $desktop = [Environment]::GetFolderPath("Desktop")
    $desktopShortcut = Join-Path $desktop "$DisplayName.lnk"
    Create-Shortcut $appExe $desktopShortcut $appExe "Volt WebUI"
    Write-Status "  Created Desktop shortcut"
}

# 6. Create uninstaller (a tiny .ps1 that re-invokes this script with -Uninstall)
$uninstallPs1 = Join-Path $InstallDir "uninstall.ps1"
$selfPath = $MyInvocation.MyCommand.Path
$uninstallBody = @"
# Uninstall script for $DisplayName
# Auto-generated by the Volt installer. Run with:
#   powershell -ExecutionPolicy Bypass -File "`$PSCommandPath" -Uninstall
& "$selfPath" -Uninstall
"@
Set-Content -Path $uninstallPs1 -Value $uninstallBody -Encoding UTF8
# Wrapper .cmd that doesn't need -File argument
$uninstallCmd = Join-Path $InstallDir "uninstall.cmd"
$uninstallCmdBody = "@echo off`r`npowershell -ExecutionPolicy Bypass -File ""$uninstallPs1"" -Uninstall`r`n"
Set-Content -Path $uninstallCmd -Value $uninstallCmdBody -Encoding ASCII
Create-Shortcut $uninstallCmd $startMenuUninstall $appExe "Uninstall $DisplayName"
Write-Status "  Created uninstaller: $uninstallCmd"

# 7. Register in Add/Remove Programs
$displayIcon = "$appExe,0"
$uninstallPs1 = Join-Path $InstallDir "uninstall.ps1"
$uninstallString = "powershell -ExecutionPolicy Bypass -File ""$uninstallPs1"" -Uninstall"
New-Item -Path $RegistryUninstallKey -Force | Out-Null
Set-ItemProperty -Path $RegistryUninstallKey -Name "DisplayName" -Value $DisplayName
Set-ItemProperty -Path $RegistryUninstallKey -Name "DisplayVersion" -Value $AppVersion
Set-ItemProperty -Path $RegistryUninstallKey -Name "Publisher" -Value $Publisher
Set-ItemProperty -Path $RegistryUninstallKey -Name "InstallLocation" -Value $InstallDir
Set-ItemProperty -Path $RegistryUninstallKey -Name "UninstallString" -Value $uninstallString
Set-ItemProperty -Path $RegistryUninstallKey -Name "DisplayIcon" -Value $displayIcon
Set-ItemProperty -Path $RegistryUninstallKey -Name "NoModify" -Value 1
Set-ItemProperty -Path $RegistryUninstallKey -Name "NoRepair" -Value 1
Write-Status "  Registered in Add/Remove Programs"

# 8. Optionally add to PATH
if (-not $NoPath) {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$currentPath;$InstallDir", "User")
        Write-Status "  Added $InstallDir to user PATH"
    }
}

Write-Status ""
Write-Status "Install complete!" -ForegroundColor Green
Write-Status "  Binary:       $appExe"
Write-Status "  Start Menu:   $startMenuShortcut"
if (-not $NoDesktopShortcut) {
    $desktop = [Environment]::GetFolderPath("Desktop")
    Write-Status "  Desktop:      $(Join-Path $desktop "$DisplayName.lnk")"
}
Write-Status "  Uninstall:    Run uninstall.cmd, or use Add/Remove Programs"
Write-Status ""
Write-Status "Before first launch, ensure your .env has GROQ_API_KEY and DATABASE_URL set." -ForegroundColor Yellow
Write-Status "A template is at: $(Join-Path $InstallDir '.env.example')"
Write-Status "Your existing .env at $(Join-Path $env:USERPROFILE '.env') (if any) will be picked up automatically."

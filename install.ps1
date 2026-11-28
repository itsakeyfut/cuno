# Installs cuno from its GitHub releases:
#
#   powershell -c "irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex"
#
# It puts cuno.exe in <cuno home>\bin and that directory on the user's PATH.
# See docs/en/specs/distribution.md, "The PowerShell one-liner". Runs on
# Windows PowerShell 5.1 and later.
param(
    # The version to install, x.y.z. The latest release when not given.
    [string]$Version = '',
    # Where releases are fetched from: a company mirror, or a test's files.
    [string]$Releases = 'https://github.com/itsakeyfut/cuno/releases'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

# `CUNO_HOME` when set, which must be absolute as cuno requires, otherwise
# %USERPROFILE%\.cuno (docs/en/specs/distribution.md, "cuno home directory").
function Get-CunoHome {
    $value = $env:CUNO_HOME
    if ([string]::IsNullOrEmpty($value)) {
        return Join-Path $env:USERPROFILE '.cuno'
    }
    # A drive and a root (C:\x), or a UNC path (\\server\share). `\x` and
    # `C:x` depend on the current drive or directory, so they are refused.
    if ($value -notmatch '^[A-Za-z]:[\\/]' -and $value -notmatch '^[\\/]{2}[^\\/]') {
        throw "CUNO_HOME must be an absolute path: $value"
    }
    return $value.TrimEnd('\', '/')
}

# Windows' own curl.exe and tar.exe, by path: PowerShell 5.1's `curl` is an
# alias of Invoke-WebRequest, and a PATH that puts Git for Windows' tools
# first finds a GNU tar that cannot read a zip.
$Curl = Join-Path $env:SystemRoot 'System32\curl.exe'
$Tar = Join-Path $env:SystemRoot 'System32\tar.exe'

# Runs curl.exe, failing on HTTP errors.
function Invoke-Curl {
    $output = & $Curl @args 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "cannot download $($args[-1]): $output"
    }
    return $output
}

# The latest version, from where `<Releases>/latest` redirects to
# (.../releases/tag/v<x.y.z>). The redirect is read from its `Location`
# header rather than with `-w '%{url_effective}'`, which some curl builds
# (Git for Windows' 8.8.0) fail with error 43.
function Get-LatestVersion {
    $headers = Invoke-Curl -fsS -D - -o NUL "$Releases/latest"
    $location = $headers | Where-Object { $_ -match '^location:\s*(\S+)' } | Select-Object -Last 1
    if (-not $location) {
        throw "$Releases/latest did not redirect to a release"
    }
    $null = $location -match '^location:\s*(\S+)'
    $tag = ($Matches[1] -split '/')[-1]
    if ($tag -notmatch '^v(\d+\.\d+\.\d+)$') {
        throw "the latest release's tag is not a version: $tag"
    }
    return $Matches[1]
}

# Downloads the release's zip and checksum into $dir, checks them, and
# returns the unpacked cuno.exe.
function Get-CunoExe([string]$ver, [string]$dir) {
    $zip = "cuno-$ver-x86_64-pc-windows-msvc.zip"
    $base = "$Releases/download/v$ver"
    Invoke-Curl -fsSL -o (Join-Path $dir $zip) "$base/$zip" | Out-Null
    Invoke-Curl -fsSL -o (Join-Path $dir "$zip.sha256") "$base/$zip.sha256" | Out-Null
    $want = ((Get-Content -Raw (Join-Path $dir "$zip.sha256")).Trim() -split '\s+')[0]
    $have = (Get-FileHash -Algorithm SHA256 (Join-Path $dir $zip)).Hash
    if ($want -ne $have) {
        throw "$zip does not match its checksum"
    }
    & $Tar -xf (Join-Path $dir $zip) -C $dir cuno.exe
    $exe = Join-Path $dir 'cuno.exe'
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $exe)) {
        throw "$zip holds no cuno.exe"
    }
    return $exe
}

# Puts $new at $bin\cuno.exe. A cuno.exe already there may be running, which
# Windows lets be renamed but not deleted, so it is moved aside first and put
# back if the copy fails.
function Install-Exe([string]$new, [string]$bin) {
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    $exe = Join-Path $bin 'cuno.exe'
    $old = Join-Path $bin 'cuno.exe.old'
    Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
    $moved = $false
    if (Test-Path -LiteralPath $exe) {
        Move-Item -LiteralPath $exe -Destination $old
        $moved = $true
    }
    try {
        Copy-Item -LiteralPath $new -Destination $exe
    } catch {
        if ($moved) { Move-Item -LiteralPath $old -Destination $exe }
        throw
    }
    Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
}

# Whether the PATH entry $entry names $dir: compared expanded, without a
# trailing backslash, ignoring case.
function Test-SameDir([string]$entry, [string]$dir) {
    $a = [Environment]::ExpandEnvironmentVariables($entry).TrimEnd('\', '/')
    return [string]::Equals($a, $dir.TrimEnd('\', '/'), [StringComparison]::OrdinalIgnoreCase)
}

# Appends $dir to the user's PATH unless it is there, keeping the value's
# unexpanded form and type, and tells running programs of the change so that
# a new terminal sees it.
function Add-UserPath([string]$dir) {
    $key = Get-Item -LiteralPath 'HKCU:\Environment'
    $path = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $entries = @($path -split ';' | Where-Object { $_ -ne '' })
    if (-not ($entries | Where-Object { Test-SameDir $_ $dir })) {
        $value = (@($entries) + $dir) -join ';'
        Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name Path -Value $value -Type ExpandString
        Send-SettingChange
    }
    if (-not (($env:Path -split ';') | Where-Object { $_ -ne '' -and (Test-SameDir $_ $dir) })) {
        $env:Path = "$env:Path;$dir"
    }
}

function Send-SettingChange {
    if (-not ('Cuno.Native' -as [type])) {
        Add-Type -Namespace Cuno -Name Native -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam,
    string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
    }
    $result = [UIntPtr]::Zero
    # HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG, 5 seconds.
    [Cuno.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
}

function Install-Cuno {
    $home_ = Get-CunoHome
    $ver = $Version
    if ($ver -eq '') {
        $ver = Get-LatestVersion
    } elseif ($ver -notmatch '^\d+\.\d+\.\d+$') {
        throw "-Version must be x.y.z: $ver"
    }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "cuno-install-$PID-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $new = Get-CunoExe $ver $tmp
        $bin = Join-Path $home_ 'bin'
        Install-Exe $new $bin
        Add-UserPath $bin
        Write-Host "Installed cuno $ver to $(Join-Path $bin 'cuno.exe')"
        Write-Host 'Open a new terminal to use cuno.'
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

try {
    Install-Cuno
} catch {
    Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red
    throw
}

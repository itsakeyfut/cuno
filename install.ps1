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

# Everything runs in this script block, so that under `irm | iex`, which runs
# the text in the caller's own session, no preference, mode, function or
# variable is left behind there.
& {
    $ErrorActionPreference = 'Stop'
    Set-StrictMode -Version 3

    # Windows' own curl.exe and tar.exe, by path: PowerShell 5.1's `curl` is
    # an alias of Invoke-WebRequest, and a PATH that puts Git for Windows'
    # tools first finds a GNU tar that cannot read a zip.
    $Curl = Join-Path $env:SystemRoot 'System32\curl.exe'
    $Tar = Join-Path $env:SystemRoot 'System32\tar.exe'

    # A directory as PATH compares it: environment variables expanded,
    # surrounding quotes and spaces removed, `/`, `.` and `..` resolved, no
    # trailing separator. $null for an entry that names no directory.
    function Get-NormalDir([string]$dir) {
        $d = [Environment]::ExpandEnvironmentVariables($dir).Trim().Trim('"').Trim()
        if ($d -eq '') { return $null }
        try { $d = [IO.Path]::GetFullPath($d) } catch { return $null }
        return $d.TrimEnd('\')
    }

    # `CUNO_HOME` when set, which must be absolute as cuno requires,
    # otherwise %USERPROFILE%\.cuno (docs/en/specs/distribution.md, "cuno
    # home directory").
    function Get-CunoHome {
        $value = $env:CUNO_HOME
        if ([string]::IsNullOrEmpty($value)) {
            return Join-Path $env:USERPROFILE '.cuno'
        }
        # A drive and a root (C:\x), or a UNC path (\\server\share). `\x`
        # and `C:x` depend on the current drive or directory, so they are
        # refused.
        if ($value -notmatch '^[A-Za-z]:[\\/]' -and $value -notmatch '^[\\/]{2}[^\\/]') {
            throw "CUNO_HOME must be an absolute path: $value"
        }
        return Get-NormalDir $value
    }

    # Runs curl.exe; a failure names the URL and curl's own message. Its
    # stderr is read as text rather than left to `Stop`, which would turn
    # its first line into the error and lose the URL.
    function Invoke-Curl {
        $url = $args[-1]
        $ErrorActionPreference = 'Continue'
        $output = & $Curl --proto '=https,file' --proto-redir '=https' @args 2>&1 |
            ForEach-Object { "$_" }
        if ($LASTEXITCODE -ne 0) {
            $why = ($output | Where-Object { $_ -match '^curl: ' } | Select-Object -Last 1)
            throw "cannot download ${url}: $why"
        }
        return $output
    }

    # The latest version, from where `<Releases>/latest` redirects to
    # (.../releases/tag/v<x.y.z>). The redirect is read from its `Location`
    # header rather than with `-w '%{url_effective}'`, which some curl builds
    # (Git for Windows' 8.8.0) fail with error 43.
    function Get-LatestVersion {
        $headers = Invoke-Curl -fsS -D - -o NUL "$Releases/latest"
        $location = $headers | Where-Object { $_ -match '^location:\s*\S' } | Select-Object -Last 1
        if (-not $location) {
            throw "$Releases/latest did not redirect to a release"
        }
        $tag = (($location -replace '^location:\s*', '').Trim() -split '/')[-1]
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
        $sum = [string](Get-Content -Raw (Join-Path $dir "$zip.sha256"))
        $want = ($sum.Trim() -split '\s+')[0]
        $have = (Get-FileHash -Algorithm SHA256 (Join-Path $dir $zip)).Hash
        if ($want -ne $have) {
            throw "$zip does not match its checksum"
        }
        & $Tar -xf (Join-Path $dir $zip) -C $dir cuno.exe 2>$null
        $exe = Join-Path $dir 'cuno.exe'
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $exe)) {
            throw "$zip holds no cuno.exe"
        }
        return $exe
    }

    # Puts $new at $bin\cuno.exe. A cuno.exe already there may be running,
    # which Windows lets be renamed but not deleted, so it is moved aside
    # first and put back if the copy fails.
    function Install-Exe([string]$new, [string]$bin) {
        if (Test-Path -LiteralPath $bin -PathType Leaf) {
            throw "$bin is a file, not a directory"
        }
        New-Item -ItemType Directory -Force -Path $bin | Out-Null
        $exe = Join-Path $bin 'cuno.exe'
        $old = Join-Path $bin 'cuno.exe.old'
        if (Test-Path -LiteralPath $old) {
            try {
                Remove-Item -LiteralPath $old -Force
            } catch {
                throw "cannot remove $old, which an earlier cuno may still be running from: close every cuno and try again"
            }
        }
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

    # Appends $dir to the user's PATH unless an entry already names it,
    # keeping the other entries as written and the value's type, and tells
    # running programs of the change so that a new terminal sees it.
    function Add-UserPath([string]$dir) {
        $key = Get-Item -LiteralPath 'HKCU:\Environment'
        $kind = 'ExpandString'
        if ($key.GetValueNames() -contains 'Path') {
            $kind = $key.GetValueKind('Path').ToString()
        }
        $path = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
        $entries = @($path -split ';' | Where-Object { $_ -ne '' })
        $want = Get-NormalDir $dir
        if (-not ($entries | Where-Object { (Get-NormalDir $_) -ieq $want })) {
            $value = (@($entries) + $want) -join ';'
            Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name Path -Value $value -Type $kind
            Send-SettingChange
        }
        if (-not (($env:Path -split ';') | Where-Object { $_ -ne '' -and (Get-NormalDir $_) -ieq $want })) {
            $env:Path = "$env:Path;$want"
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
        if ($Releases -notmatch '^(https|file)://') {
            throw "-Releases must be an https:// or file:// URL: $Releases"
        }
        $Releases = $Releases.TrimEnd('/')
        $cunoHome = Get-CunoHome
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
            $bin = Join-Path $cunoHome 'bin'
            Install-Exe $new $bin
            Add-UserPath $bin
            Write-Host "Installed cuno $ver to $(Join-Path $bin 'cuno.exe')"
            Write-Host 'Open a new terminal to use cuno.'
        } finally {
            Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    # One error, stated once: `throw` ends `powershell -c "irm ... | iex"`
    # and `powershell -File` with exit code 1, and stops an interactive
    # `irm ... | iex` without closing the window.
    try {
        Install-Cuno
    } catch {
        throw "error: $($_.Exception.Message)"
    }
}

<# : batch part (PowerShell reads this block as a comment)
@echo off
title Tachyon Mabar - Tailscale + Tachyon
set "TACHYON_BAT=%~f0"
powershell -NoProfile -ExecutionPolicy Bypass -Command "iex ([IO.File]::ReadAllText($env:TACHYON_BAT))"
echo.
pause
exit /b
#>

# ---------------------------------------------------------------------------------------------------------------------
# Tachyon Mabar: memasang Tailscale (jaringan pribadi gratis, tanpa server), menyambungkan PC ini ke jaringan
# yang sama dengan teman, mengizinkan Tachyon di firewall, lalu mengunduh dan membuka Tachyon versi Windows terbaru.
# Jalankan lagi kapan saja: yang sudah terpasang dilewati, Tachyon diperbarui kalau ada versi baru.
# ---------------------------------------------------------------------------------------------------------------------
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo     = 'Rxuser2/Tachyon'
$Asset    = 'Tachyon-windows.zip'
$Home_    = Join-Path $env:LOCALAPPDATA 'Tachyon'
$AppDir   = Join-Path $Home_ 'app'
$Stamp    = Join-Path $Home_ 'versi.txt'
$TsExe    = Join-Path $env:ProgramFiles 'Tailscale\tailscale.exe'
# TACHYON_MABAR_AUTO=1: no questions, each answer its default (the CI test of this file).
$Auto     = $env:TACHYON_MABAR_AUTO -eq '1'

function Ask($q, $default) { if ($Auto) { Write-Host "   $q $default"; return $default }; return Read-Host "   $q" }
function Step($t) { Write-Host ''; Write-Host "== $t" -ForegroundColor Cyan }
function Ok($t)   { Write-Host "   $t" -ForegroundColor Green }
function Info($t) { Write-Host "   $t" }
function Warn($t) { Write-Host "   $t" -ForegroundColor Yellow }

# Runs a script block as administrator (one UAC prompt), waiting for it.
function As-Admin([string]$script) {
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $enc)
    return $p.ExitCode
}

# Tailscale's IPv4 address, or nothing (its error output must not stop the script).
function Ts-Ip {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'SilentlyContinue'
    $r = & $TsExe ip -4 2>$null | Select-Object -First 1
    $ErrorActionPreference = $old
    return $r
}

try {
    New-Item -ItemType Directory -Force -Path $Home_ | Out-Null

    # ---- 1. Tachyon: the newest Windows release -----------------------------------------------------------------
    Step '1/4  Mencari Tachyon versi Windows terbaru'
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=30" `
        -Headers @{ 'User-Agent' = 'Tachyon-Mabar' }
    $rel = $releases | Where-Object { $_.tag_name -like 'tachyon-windows-*' -and ($_.assets.name -contains $Asset) } |
        Select-Object -First 1
    if (-not $rel) { throw "Tidak ada rilis Windows di github.com/$Repo (repo harus public)." }
    $tag = $rel.tag_name
    $url = ($rel.assets | Where-Object { $_.name -eq $Asset } | Select-Object -First 1).browser_download_url
    $have = if (Test-Path $Stamp) { (Get-Content $Stamp -Raw).Trim() } else { '' }
    $exe = Get-ChildItem -Path $AppDir -Filter 'Tachyon.exe' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($have -eq $tag -and $exe) {
        Ok "Sudah versi terbaru: $tag"
    } else {
        Info "Mengunduh $tag (sekitar 500 MB, bisa lama)..."
        $zip = Join-Path $Home_ $Asset
        # curl.exe (bawaan Windows 10/11) menampilkan progres dan bisa melanjutkan unduhan yang terputus.
        & curl.exe -L --fail --retry 5 -C - -o $zip $url
        if ($LASTEXITCODE -ne 0) {
            Remove-Item $zip -ErrorAction SilentlyContinue
            & curl.exe -L --fail --retry 5 -o $zip $url
            if ($LASTEXITCODE -ne 0) { throw "Unduhan gagal (curl $LASTEXITCODE)." }
        }
        Info 'Mengekstrak...'
        if (Test-Path $AppDir) { Remove-Item $AppDir -Recurse -Force }
        New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
        & tar.exe -xf $zip -C $AppDir
        if ($LASTEXITCODE -ne 0) { Expand-Archive -Path $zip -DestinationPath $AppDir -Force }
        Remove-Item $zip -ErrorAction SilentlyContinue
        Set-Content -Path $Stamp -Value $tag
        $exe = Get-ChildItem -Path $AppDir -Filter 'Tachyon.exe' -Recurse | Select-Object -First 1
        if (-not $exe) { throw 'Tachyon.exe tidak ditemukan di zip.' }
        Ok "Tachyon $tag terpasang di $($exe.DirectoryName)"
    }
    $exePath = $exe.FullName

    # Shortcut on the desktop.
    $lnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Tachyon.lnk'
    $sh = New-Object -ComObject WScript.Shell
    $s = $sh.CreateShortcut($lnk)
    $s.TargetPath = $exePath
    $s.WorkingDirectory = $exe.DirectoryName
    $s.Save()
    Ok 'Shortcut "Tachyon" dibuat di Desktop.'

    # ---- 2. Tailscale and the firewall (administrator) ----------------------------------------------------------
    Step '2/4  Tailscale dan firewall (Windows akan meminta izin administrator, klik Yes)'
    $msi = Join-Path $env:TEMP 'tailscale-setup-latest-amd64.msi'
    $needTs = -not (Test-Path $TsExe)
    if ($needTs) {
        Info 'Mengunduh Tailscale...'
        & curl.exe -L --fail --retry 5 -o $msi 'https://pkgs.tailscale.com/stable/tailscale-setup-latest-amd64.msi'
        if ($LASTEXITCODE -ne 0) { throw "Unduhan Tailscale gagal (curl $LASTEXITCODE)." }
    } else {
        Ok 'Tailscale sudah terpasang.'
    }
    $admin = @"
`$ErrorActionPreference = 'Continue'
if ('$needTs' -eq 'True') { Start-Process msiexec.exe -Wait -ArgumentList '/i', '"$msi"', '/quiet', '/norestart' }
netsh advfirewall firewall delete rule name="Tachyon" | Out-Null
netsh advfirewall firewall add rule name="Tachyon" dir=in action=allow program="$exePath" enable=yes profile=any | Out-Null
netsh advfirewall firewall add rule name="Tachyon (Play with Friends)" dir=in action=allow protocol=TCP localport=25580 profile=any | Out-Null
"@
    As-Admin $admin | Out-Null
    if (-not (Test-Path $TsExe)) { throw 'Tailscale belum terpasang (izin administrator ditolak?). Jalankan file ini lagi.' }
    Ok 'Tailscale terpasang; Tachyon diizinkan di firewall (port 25580).'

    # ---- 3. Join the tailnet ------------------------------------------------------------------------------------
    Step '3/4  Menyambung ke jaringan Tailscale'
    $ip = (Ts-Ip)
    if ($ip) {
        Ok "PC ini sudah tersambung ke Tailscale: $ip"
        $again = Ask 'Ganti ke jaringan teman dengan auth key? (y/N)' 'n'
    }
    if (-not $ip -or $again -match '^[yY]') {
        Write-Host ''
        Write-Host '   HOST (yang membuka world): kosongkan, tekan Enter, lalu login di browser.' -ForegroundColor White
        Write-Host '   TEMAN: tempel auth key yang dikirim host (diawali tskey-auth-).' -ForegroundColor White
        $key = ([string](Ask 'Auth key:' $env:TACHYON_MABAR_KEY)).Trim()
        if ($key) {
            & $TsExe up --authkey=$key --force-reauth --reset --unattended
            if ($LASTEXITCODE -ne 0) { Warn "tailscale up keluar dengan kode $LASTEXITCODE" }
        } elseif ($Auto) {
            Warn 'Tanpa auth key: login dilewati (mode otomatis).'
        } else {
            Info 'Browser akan terbuka untuk login Tailscale (Google/Microsoft/GitHub)...'
            & $TsExe up --unattended
            if ($LASTEXITCODE -ne 0) { Warn "tailscale up keluar dengan kode $LASTEXITCODE" }
        }
        Start-Sleep -Seconds 3
        $ip = (Ts-Ip)
    }
    if ($ip) { Ok "Alamat Tailscale PC ini: $ip" } else { Warn 'Belum tersambung. Buka ikon Tailscale di taskbar dan login, lalu jalankan file ini lagi.' }

    # ---- 4. How to play together --------------------------------------------------------------------------------
    Step '4/4  Cara mabar'
    Write-Host @'
   HOST, sekali saja:
     - Buka https://login.tailscale.com/admin/settings/keys  ->  "Generate auth key"
       centang "Reusable", lalu kirim key itu ke teman (rahasiakan, seperti password).
     - Untuk HP Android: pasang aplikasi Tailscale dari Play Store dan login dengan akun yang sama.

   Setiap main:
     - HOST: buka world di Tachyon -> Esc -> tombol Friends -> Start Sharing -> salin kode undangan.
     - TEMAN: Tachyon -> title screen -> Friends -> Join a Friend -> tempel kode -> Join.
     - Kedua pihak harus memakai versi Tachyon yang sama (jalankan file ini untuk update).
     - Create: pasang dulu di kedua perangkat (Tachyon -> Mods), mod bawaan lain sudah ada.
'@
    $run = Ask 'Buka Tachyon sekarang? (Y/n)' $(if ($Auto) { 'n' } else { 'y' })
    if ($run -notmatch '^[nN]') { Start-Process -FilePath $exePath -WorkingDirectory $exe.DirectoryName }
    Ok 'Selesai.'
} catch {
    Write-Host ''
    Write-Host "GAGAL: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}

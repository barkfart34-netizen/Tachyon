<# : batch part (PowerShell reads this block as a comment)
@echo off
title Tachyon Mabar - Setting
net session >nul 2>&1
if errorlevel 1 (
  echo Meminta izin administrator ^(untuk firewall^)... klik Yes.
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
set "TACHYON_BAT=%~f0"
powershell -NoProfile -ExecutionPolicy Bypass -Command "iex ([IO.File]::ReadAllText($env:TACHYON_BAT))"
echo.
pause
exit /b
#>

# ---------------------------------------------------------------------------------------------------------------------
# Tachyon Mabar - Setting: TIDAK mengunduh apa pun. Taruh file ini di folder game (Tachyon atau Tachyon-Orbital) lalu
# klik dua kali. Ia mencari Tachyon.exe / horizon-client.exe, mengizinkannya di Windows Firewall, menyambungkan
# Tailscale yang sudah terpasang, dan bisa mengetes sambungan ke PC host.
# ---------------------------------------------------------------------------------------------------------------------
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Here  = Split-Path -Parent $env:TACHYON_BAT
$TsExe = Join-Path $env:ProgramFiles 'Tailscale\tailscale.exe'
# TACHYON_MABAR_AUTO=1: no questions, each answer its default (the CI test of this file).
$Auto  = $env:TACHYON_MABAR_AUTO -eq '1'

function Step($t) { Write-Host ''; Write-Host "== $t" -ForegroundColor Cyan }
function Ok($t)   { Write-Host "   $t" -ForegroundColor Green }
function Info($t) { Write-Host "   $t" }
function Warn($t) { Write-Host "   $t" -ForegroundColor Yellow }
function Ask($q, $default) { if ($Auto) { Write-Host "   $q $default"; return $default }; return Read-Host "   $q" }
function Ts-Ip {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'SilentlyContinue'
    $r = & $TsExe ip -4 2>$null | Select-Object -First 1
    $ErrorActionPreference = $old
    return $r
}
function Find-Games($root) {
    if (-not $root -or -not (Test-Path -LiteralPath $root)) { return @() }
    return @(Get-ChildItem -LiteralPath $root -Recurse -Depth 4 -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -eq 'Tachyon.exe' -or $_.Name -eq 'horizon-client.exe' })
}

try {
    # ---- 1. The game -------------------------------------------------------------------------------------------
    Step '1/4  Mencari game di folder ini'
    $games = @(Find-Games $Here) + @(Find-Games (Join-Path $env:LOCALAPPDATA 'Tachyon\app'))
    while ($games.Count -eq 0) {
        Warn "Tachyon.exe / horizon-client.exe tidak ada di $Here"
        $dir = Ask 'Tempel alamat folder game (kosongkan untuk lewati firewall):' ''
        if (-not $dir) { break }
        $games = @(Find-Games ($dir.Trim().Trim('"')))
    }
    $games = @($games | Sort-Object FullName -Unique)
    foreach ($g in $games) { Ok "Ketemu: $($g.FullName)" }

    # ---- 2. Windows Firewall -----------------------------------------------------------------------------------
    Step '2/4  Mengizinkan game di Windows Firewall'
    foreach ($g in $games) {
        $name = "Tachyon Mabar: $($g.FullName)"
        Remove-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
        New-NetFirewallRule -DisplayName $name -Direction Inbound -Action Allow -Program $g.FullName -Profile Any | Out-Null
        Ok "Diizinkan: $($g.Name)"
    }
    $port = 'Tachyon Mabar: port 25580 (Play with Friends)'
    Remove-NetFirewallRule -DisplayName $port -ErrorAction SilentlyContinue
    New-NetFirewallRule -DisplayName $port -Direction Inbound -Action Allow -Protocol TCP -LocalPort 25580 -Profile Any | Out-Null
    Ok 'Port TCP 25580 dibuka.'

    # ---- 3. Tailscale ------------------------------------------------------------------------------------------
    Step '3/4  Tailscale'
    $ip = $null
    if (-not (Test-Path $TsExe)) {
        Warn 'Tailscale belum terpasang. Pasang dari https://tailscale.com/download/windows lalu jalankan file ini lagi.'
    } else {
        $ip = Ts-Ip
        $change = 'n'
        if ($ip) {
            Ok "Sudah tersambung. Alamat Tailscale PC ini: $ip"
            $change = Ask 'Pindah ke jaringan host dengan auth key? (y/N)' 'n'
        }
        if (-not $ip -or $change -match '^[yY]') {
            Write-Host ''
            Write-Host '   HOST: kosongkan lalu Enter, login di browser yang terbuka.' -ForegroundColor White
            Write-Host '   TEMAN: tempel auth key dari host (diawali tskey-auth-), atau kosongkan untuk login sendiri' -ForegroundColor White
            Write-Host '          lalu terima undangan Share dari host.' -ForegroundColor White
            $key = ([string](Ask 'Auth key:' $env:TACHYON_MABAR_KEY)).Trim()
            if ($key) {
                & $TsExe up --authkey=$key --force-reauth --reset --unattended
                if ($LASTEXITCODE -ne 0) { Warn "tailscale up keluar dengan kode $LASTEXITCODE" }
            } elseif ($Auto) {
                Warn 'Login dilewati (mode otomatis).'
            } else {
                & $TsExe up --unattended
                if ($LASTEXITCODE -ne 0) { Warn "tailscale up keluar dengan kode $LASTEXITCODE" }
            }
            Start-Sleep -Seconds 3
            $ip = Ts-Ip
            if ($ip) { Ok "Alamat Tailscale PC ini: $ip" } else { Warn 'Belum tersambung: buka ikon Tailscale di taskbar dan login.' }
        }
    }

    # ---- 4. A test to the host ---------------------------------------------------------------------------------
    Step '4/4  Tes sambungan ke host (khusus TEMAN)'
    $peer = ([string](Ask 'IP Tailscale host 100.x.x.x (kosongkan kalau Anda host):' '')).Trim()
    if ($peer -and (Test-Path $TsExe)) {
        & $TsExe ping --c=3 $peer
        if ($LASTEXITCODE -eq 0) { Ok 'Host terjangkau lewat Tailscale.' } else { Warn 'Host tidak terjangkau: cek keduanya Connected dan Share sudah diterima.' }
        $open = Test-NetConnection -ComputerName $peer -Port 25580 -InformationLevel Quiet -WarningAction SilentlyContinue
        if ($open) { Ok 'Port 25580 host terbuka: host sedang Start Sharing, siap Join.' }
        else { Info 'Port 25580 host tertutup: wajar kalau host belum Start Sharing; kalau sudah, jalankan file ini di PC host.' }
    }

    Write-Host @'

   Cara mabar:
     HOST : buka world -> Esc -> ikon Friends -> Start Sharing -> Copy Code -> kirim ke teman.
     TEMAN: Multiplayer -> Join a Friend -> tempel kode -> Join.
     Keduanya memakai versi game yang sama, dan nama pemain berbeda.
'@
    Ok 'Setting selesai.'
} catch {
    Write-Host ''
    Write-Host "GAGAL: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}

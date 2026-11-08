@echo off
rem Tachyon Orbital: Horizon's Orbital Laser fork (Stink-O/horizon, MIT) with Orbital Laser 0.5.11 and The Fold,
rem the resources already prepared. Offline: no account, just a player name, asked once.
setlocal
set "HERE=%~dp0"
set "RES=%HERE%horizon\.local\horizon-client\26.3"
if not exist "%RES%\horizon-resources.json" (
  echo Resource game tidak ditemukan di %RES%
  echo Ekstrak ulang zip Tachyon-Orbital-windows.zip dengan lengkap.
  pause
  exit /b 1
)
if not exist "%HERE%game" mkdir "%HERE%game"
set "NAMEFILE=%HERE%game\nama-pemain.txt"
if exist "%NAMEFILE%" goto have_name

:ask_name
set "PNAME="
set /p "PNAME=Nama pemain (offline, 3-16 huruf/angka/_): "
if "%PNAME%"=="" goto ask_name
>"%NAMEFILE%" echo %PNAME%

:have_name
set /p PNAME=<"%NAMEFILE%"
echo Bermain sebagai %PNAME%  (ganti nama: hapus game\nama-pemain.txt)
set "HORIZON_NO_HEARTBEAT=1"
cd /d "%HERE%game"
start "" "%HERE%horizon\horizon-client.exe" --resources "%RES%" --name "%PNAME%"

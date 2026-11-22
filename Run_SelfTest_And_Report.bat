@echo off
setlocal
title PercyScarlett Self-Test ^& GitHub Issue Reporter
cd /d "%~dp0"

set "EXE=%~dp0percyscarlett.exe"
if not exist "%EXE%" if exist "%~dp0target\release\percyscarlett.exe" set "EXE=%~dp0target\release\percyscarlett.exe"

set "HAS_DLL=0"
if exist "%~dp0d3d12_xs.dll" set "HAS_DLL=1"
if exist "%LOCALAPPDATA%\PercyScarlett\runtime\d3d12_xs.dll" set "HAS_DLL=1"

if not exist "%EXE%" set "NEED_ZIP=1"
if "%HAS_DLL%"=="0" set "NEED_ZIP=1"

if "%NEED_ZIP%"=="1" (
    echo Downloading latest PercyScarlett Win64 runtime bundle from GitHub Releases...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "$rel = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/rpatty-dev/PercyScarlett/releases/latest'; $zip = ($rel.assets | Where-Object { $_.name -like '*Win64.zip' } | Select-Object -First 1).browser_download_url; Invoke-WebRequest -UseBasicParsing $zip -OutFile '%~dp0PercyScarlett_latest.zip'; Expand-Archive -Force '%~dp0PercyScarlett_latest.zip' -DestinationPath '%~dp0'; Remove-Item -Force '%~dp0PercyScarlett_latest.zip'"
    set "EXE=%~dp0percyscarlett.exe"
)

if not exist "%EXE%" (
    echo [ERROR] Could not find or download percyscarlett.exe
    pause
    exit /b 1
)

echo Running PercyScarlett Self-Test and opening pre-filled GitHub Issue...
echo.
"%EXE%" self-test --open
echo.
echo Done! Your browser should open with a pre-filled GitHub Issue.
pause

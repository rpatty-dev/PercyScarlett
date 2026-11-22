@echo off
setlocal
title PercyScarlett Self-Test ^& GitHub Issue Reporter
cd /d "%~dp0"

set "EXE=%~dp0percyscarlett.exe"
if not exist "%EXE%" if exist "%~dp0target\release\percyscarlett.exe" set "EXE=%~dp0target\release\percyscarlett.exe"
if not exist "%EXE%" (
    echo Downloading latest percyscarlett.exe from GitHub Releases...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-WebRequest -UseBasicParsing 'https://github.com/rpatty-dev/PercyScarlett/releases/latest/download/percyscarlett.exe' -OutFile '%~dp0percyscarlett.exe'"
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

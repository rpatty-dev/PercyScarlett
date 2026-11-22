@echo off
setlocal EnableDelayedExpansion
title PercyScarlett Self-Test ^& GitHub Issue Reporter
cd /d "%~dp0"

set "RT=%LOCALAPPDATA%\PercyScarlett\runtime"
set "EXE=%~dp0percyscarlett.exe"
if not exist "%EXE%" if exist "%~dp0target\release\percyscarlett.exe" set "EXE=%~dp0target\release\percyscarlett.exe"
if not exist "%EXE%" if exist "%RT%\percyscarlett.exe" set "EXE=%RT%\percyscarlett.exe"

set "HAS_DLL=0"
if exist "%~dp0d3d12_xs.dll" if exist "%~dp0dxcompiler.dll" set "HAS_DLL=1"
if exist "%RT%\d3d12_xs.dll" if exist "%RT%\dxcompiler.dll" set "HAS_DLL=1"

set "NEED_ZIP=0"
if not exist "%EXE%" set "NEED_ZIP=1"
if "%HAS_DLL%"=="0" set "NEED_ZIP=1"

if "%NEED_ZIP%"=="1" (
    echo [1/3] Downloading latest PercyScarlett Win64 runtime bundle...
    set "ZIP=%~dp0PercyScarlett_latest.zip"
    if exist "!ZIP!" del /f /q "!ZIP!"

    rem Tier 1: curl HEAD redirect (no GitHub API rate limit)
    for /f "delims=" %%U in ('curl.exe -fsSIL -o NUL -w "%%{url_effective}" "https://github.com/rpatty-dev/PercyScarlett/releases/latest" 2^>nul') do set "EFF=%%U"
    for %%T in ("!EFF:/tag/= !") do set "TAG=%%~nxT"
    if defined TAG (
        set "ZURL=https://github.com/rpatty-dev/PercyScarlett/releases/download/!TAG!/PercyScarlett_!TAG!_Win64.zip"
        curl.exe -fL --retry 2 -o "!ZIP!" "!ZURL!" 2>nul
    )

    rem Tier 2: PowerShell TLS 1.2 + GitHub API fallback
    if not exist "!ZIP!" (
        powershell -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; $rel = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/rpatty-dev/PercyScarlett/releases/latest'; $zip = ($rel.assets | Where-Object { $_.name -like '*Win64.zip' } | Select-Object -First 1).browser_download_url; Invoke-WebRequest -UseBasicParsing $zip -OutFile '%~dp0PercyScarlett_latest.zip'" 2>nul
    )

    if exist "!ZIP!" (
        echo [2/3] Extracting runtime bundle...
        tar -xf "!ZIP!" -C "%~dp0" 2>nul
        if errorlevel 1 (
            powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -Force '%~dp0PercyScarlett_latest.zip' -DestinationPath '%~dp0'" 2>nul
        )
        del /f /q "!ZIP!" 2>nul
        set "EXE=%~dp0percyscarlett.exe"
    )

    rem Tier 3: standalone percyscarlett.exe fallback
    if not exist "%EXE%" (
        echo [2/3] Zip failed, falling back to standalone percyscarlett.exe...
        curl.exe -fL -o "%~dp0percyscarlett.exe" "https://github.com/rpatty-dev/PercyScarlett/releases/latest/download/percyscarlett.exe" 2>nul
        if not exist "%~dp0percyscarlett.exe" certutil -urlcache -split -f "https://github.com/rpatty-dev/PercyScarlett/releases/latest/download/percyscarlett.exe" "%~dp0percyscarlett.exe" >nul 2>&1
        set "EXE=%~dp0percyscarlett.exe"
    )
)

if not exist "%EXE%" (
    echo [ERROR] Could not find or download percyscarlett.exe. Check internet/firewall.
    pause
    exit /b 1
)

set "PATH=%~dp0;%RT%;%PATH%"
echo [3/3] Running PercyScarlett Self-Test and opening pre-filled GitHub Issue...
echo.
"%EXE%" self-test --open
echo.
echo Report saved to: "%~dp0percyscarlett_test_report.md"
echo If your browser did not open automatically, copy the URL above.
pause

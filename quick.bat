@echo off
chcp 65001 >nul 2>&1
cd /d "%~dp0"
echo ============================================================
echo   RUOO-CONSOLE // Quick Start (no build, no AV delay)
echo ============================================================
echo.
echo [*] Setting RUOO_NO_EVASION=1 to skip sandbox delay...
set "RUOO_NO_EVASION=1"

set "EXE="
if exist "target\debug\ruoo-console.exe" set "EXE=target\debug\ruoo-console.exe"
if exist "target\release\ruoo-console.exe" set "EXE=target\release\ruoo-console.exe"

if not "%EXE%"=="" goto :found
echo [!] No exe found. Building...
cargo build
if errorlevel 1 (
    echo [!] Build failed!
    pause
    exit /b 1
)
if exist "target\debug\ruoo-console.exe" set "EXE=target\debug\ruoo-console.exe"
if exist "target\release\ruoo-console.exe" set "EXE=target\release\ruoo-console.exe"

:found
echo [*] EXE: %EXE%
echo [*] RUOO_NO_EVASION=1
echo.

"%EXE%" 2>&1

echo.
echo [*] Exit code: %errorlevel%
pause

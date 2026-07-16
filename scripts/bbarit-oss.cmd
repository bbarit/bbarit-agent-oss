@echo off
title bbarit-oss
rem Double-click launcher: opens a folder picker, then the bbarit TUI.
rem Uses the full path so it works regardless of PATH in the launch context.
set "BBARIT=%USERPROFILE%\.cargo\bin\bbarit-oss.exe"
if not exist "%BBARIT%" (
  echo Could not find bbarit-oss at "%BBARIT%".
  echo Install it with:  cargo install --path .
  echo.
  pause
  exit /b 1
)
echo Starting bbarit...  (a folder picker will open; check the taskbar if it's behind this window)
"%BBARIT%" %*
echo.
echo [bbarit exited with code %errorlevel%]
pause

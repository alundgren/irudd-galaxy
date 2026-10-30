@echo off
rem The Lantern Reach: build in release (thin LTO) and play.
rem Arguments go to `galaxy play`, e.g. play.cmd --new
setlocal
cd /d "%~dp0"
cargo build --release -p galaxy-app || exit /b 1
"target\release\galaxy.exe" play %*

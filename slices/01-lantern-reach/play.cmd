@echo off
rem The Lantern Reach: build in release (thin LTO) and play.
rem Arguments go to `galaxy play`, e.g. play.cmd --new
setlocal
cd /d "%~dp0"
cargo xtask run --release -- play %*

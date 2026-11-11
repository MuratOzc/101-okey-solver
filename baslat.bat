@echo off
cd /d "%~dp0dist"
start "" http://localhost:8101
echo 101 Cozucu calisiyor: http://localhost:8101
echo Kapatmak icin bu pencereyi kapat.
python -m http.server 8101 --bind 127.0.0.1

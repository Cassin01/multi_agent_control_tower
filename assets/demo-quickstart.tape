Output assets/demo-quickstart.gif

Require macot
Require tmux
Require claude
Require ffmpeg

Set Shell "bash"
Set FontSize 22
Set Width 1100
Set Height 660
Set Padding 16
Set Framerate 24
Set PlaybackSpeed 2.5
Set TypingSpeed 40ms
Set Theme "Aardvark Blue"
Set WindowBar Rings
Set BorderRadius 10

Hide
Type "macot down --force --cleanup >/dev/null 2>&1 || true" Enter
Type "clear" Enter
Show

Type "macot start . -n 1" Enter
Sleep 1.8s

Type "macot tower" Enter
Sleep 1.4s
Type "Summarize README and propose one concrete improvement." Enter
Sleep 200ms
Ctrl+S
Sleep 2.4s
Ctrl+Q
Sleep 300ms

Type "macot status" Enter
Sleep 1.1s

Hide
Type "macot down --force --cleanup >/dev/null 2>&1 || true" Enter
Show

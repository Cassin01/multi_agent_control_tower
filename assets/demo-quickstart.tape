Output assets/demo-quickstart.gif

Require macot
Require tmux
Require claude
Require ffmpeg

Set Shell "bash"

Set FontSize 20
Set Width 1400
Set Height 1000

# Set Width 1100
# Set Height 660
Set FontFamily "Hack Nerd Font"
Set Padding 16
Set Framerate 24
Set PlaybackSpeed 1.5
Set TypingSpeed 40ms
Set Theme "Catppuccin Mocha"
Set WindowBar Rings
Set BorderRadius 10

Hide
Type "export PS1='$ '" Enter
Type "export PROMPT_COMMAND=''" Enter
Type "rm -rf /tmp/macot-demo && git clone --quiet . /tmp/macot-demo && cd /tmp/macot-demo" Enter
Type "macot down --force --cleanup >/dev/null 2>&1 || true" Enter
Type "clear" Enter
Show

Type "macot start . -n 2" Enter
Sleep 1.8s

Type "macot tower" Enter
Sleep 1.8s
Down
Sleep 2s
Down
Sleep 2s
Type "Summarize README and propose one concrete improvement." Enter
Sleep 200ms
Ctrl+S

Up
Sleep 1.8s
Ctrl+O
Sleep 1.8s
Down
Sleep 1.8s
Down
Sleep 1.8s
Down
Sleep 1.8s
Down
Sleep 1.8s
Enter
Sleep 15.4s
Down
Sleep 15.4s


Ctrl+Q
Sleep 300ms

Type "macot status" Enter
Sleep 1.1s

Hide
Type "macot down --force --cleanup >/dev/null 2>&1 || true" Enter
Show

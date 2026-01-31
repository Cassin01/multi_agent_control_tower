# Multi Agent Controll Tower

コマンド名: macot

## cinfig

```config
var N
var project_path
```

## macot start
1. フォルダを指定してtmuxのペインN個を起動
2. 最初のペインを作成

```sh
tmux new-session -s agent -d -c {project_path}

for i in `seq 1 $N`; do
    `tmux split-window -t "agent:0" -c {project_path}`
done

PANE_TITLES=`よしなに`
PANE_COLORS=("red" "blue" "blue" "blue" "blue" "blue" "blue" "blue" "blue")

for i in {0..N}; do
    tmux select-pane -t "multiagent:0.$i" -T "${PANE_TITLES[$i]}"
    PROMPT_STR=$(generate_prompt "${PANE_TITLES[$i]}" "${PANE_COLORS[$i]}" "$SHELL_SETTING")
    tmux send-keys -t "multiagent:0.$i" "cd \"$(pwd)\" && export PS1='${PROMPT_STR}' && clear" Enter
done

if ! command -v claude &> /dev/null; then
    echo "Claude CLI is not installed. Please install it from
fi

for i in {0..N}; do
  tmux send-keys -t "multiagent:0.0" "claude --dengerously-skip-permissions"
done

# 待機
for i in {0..30}; do
    if tmux capture-pane -t "multiagent:0.0" -p | grep -q "while true"; then
        echo "Pane $i is ready."
        break
    fi
    sleep 1
done

```

## macot stop

stop all panes and close the session


## macot tower

ユーザ入力画面を起動

命令を送る場所を選択するためのUIを提供する
ユーザの入力を受け取り、指定されたペインに命令を送信する



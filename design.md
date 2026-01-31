# Multi Agent Control Tower

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
tmux new-session -s expert -d -c {project_path}

for i in `seq 1 $N`; do
    `tmux split-window -t "expert:0" -c {project_path}`
done

PANE_TITLES=`よしなに`
PANE_COLORS=("red" "blue" "blue" "blue" "blue" "blue" "blue" "blue" "blue")

for i in {0..N}; do
    tmux select-pane -t "expert:0.$i" -T "${PANE_TITLES[$i]}"
    PROMPT_STR=$(generate_prompt "${PANE_TITLES[$i]}" "${PANE_COLORS[$i]}" "$SHELL_SETTING")
    tmux send-keys -t "expert:0.$i" "cd \"$(pwd)\" && export PS1='${PROMPT_STR}' && clear" Enter
done

if ! command -v claude &> /dev/null; then
    echo "Claude CLI is not installed. Please install it from
fi

for i in {0..N}; do
  tmux send-keys -t "expert:0.0" "claude --dengerously-skip-permissions"
done

# 待機
for i in {0..30}; do
  ready=true
  for j in {0..N}; do
    if ! tmux capture-pane -t "expert:0.0" -p | grep -q "while true"; then
      redy=false
    fi
  done
  if $ready; then
      echo "All panes are ready."
      break
  fi
  sleep 1
done

# send instruction prompt

for i in {0..N}; do
  tmux send-keys -t "expert:0.$i" "read ./instructions/${PANE_TITLES[$i]}.md"
  sleep 0.3
  tmux send-keys -t "expert:0.$i" Enter
  sleep 0.5
done


```

## macot stop

stop all panes and close the session


## macot tower

if not running, exit with error

ユーザ入力画面を起動

命令を送る場所を選択するためのUIを提供する
ユーザの入力を受け取り、指定されたペインに命令を送信する


## instructions/core

```
workflow:
  - step: 1
    action: receive_wakeup
    from: control_tower
    via: send-keys
  - step: 2
    action: read_yaml
    target: "queue/tasks/expert{ID}.yaml"
    note: "自分専用ファイルのみ"
  - step: 3
    action: update_status
    value: in_progress
  - step: 4
    action: execute_task
  - step: 5
    action: write_report
    target: "queue/reports/expert{ID}_report.yaml"
  - step: 6
    action: notify_control_tower
    command: say "{expoert_name} has completed the task."
  - step: 7
    action: update_status
    value: done

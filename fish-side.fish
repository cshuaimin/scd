set RECV_FIFO /tmp/terminal-sidebar-recv-fifo
set SEND_FILE /tmp/terminal-sidebar-send

function sync-cwd-to-fish --on-signal SIGUSR1
    cd (cat $SEND_FILE)
    commandline -f repaint
end

function sync-cwd-to-scd --on-variable PWD
    echo "cd $PWD" > $RECV_FIFO
end

function unregister --on-event fish_exit
    echo "fish_exit" > $RECV_FIFO
end

echo $fish_pid > $RECV_FIFO
sync-cwd-to-scd 
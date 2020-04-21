set RECV_FIFO /tmp/scd-recv-fifo
set SEND_FILE /tmp/scd-send

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

if test ! -p $RECV_FIFO
    if test -e $RECV_FIFO
        rm $RECV_FIFO
    end
    mkfifo $RECV_FIFO
end

echo $fish_pid > $RECV_FIFO

sync-cwd-to-scd 
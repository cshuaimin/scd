function scd_eval --on-signal SIGUSR1
    eval (scd get-cmd)
end

function scd_run_silently
    eval $argv && commandline -f repaint
end

function scd_run_with_echo
    commandline $argv && commandline -f execute
end

function scd_cd --on-variable PWD
    scd cd $PWD
end

function scd_exit --on-event fish_exit
    scd exit
end

function scd_send_task
    scd send-task $argv
    echo
    commandline ''
    echo 'Task sent to scd.'
    commandline -f repaint
end

function scd_enter_key
    set cmd (string trim --right --chars % (commandline))
    and scd_send_task $cmd
    or commandline -f execute
end

bind \r scd_enter_key
bind \cj 'scd_send_task (commandline)'

scd send-pid $fish_pid
scd_cd

function scd_deinit
    functions --erase scd_eval scd_run_silently scd_run_with_echo scd_cd scd_exit scd_deinit
end
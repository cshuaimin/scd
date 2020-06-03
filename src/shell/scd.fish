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
    set rendered (echo $argv | fish_indent --ansi)
    scd send-task $argv "$rendered"
    echo "- cmd:" $argv % >> ~/.local/share/fish/fish_history
    echo "  when:" (date "+%s") >> ~/.local/share/fish/fish_history
    history --merge
    echo
    commandline ''
    echo 'Task sent to scd.'
    commandline -f repaint
end

function scd_enter_key
    set cmd (string trim --right (commandline))
    set cmd (string trim --right --chars '% ' $cmd)
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
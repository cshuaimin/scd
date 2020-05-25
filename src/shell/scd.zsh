TRAPUSR1() {
    eval $(scd get-cmd)
}

scd_run_silently() {
    eval $@ && zle reset-prompt
}

scd_run_with_echo() {
    echo $@ && eval $@ && print -s $@ && zle reset-prompt
}

scd_cd() {
    scd cd $PWD
}

scd_exit() {
    scd exit
}

autoload add-zsh-hook
add-zsh-hook chpwd scd_cd
add-zsh-hook zshexit scd_exit
scd send-pid $$

scd_deinit() {
    add-zsh-hook -d chpwd scd_cd
    add-zsh-hook -d zshexit scd_exit
    unfunction TRAPUSR1 scd_run_silently scd_run_with_echo scd_cd scd_exit scd_deinit
}
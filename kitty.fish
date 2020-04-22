kitty @ goto-layout tall
set id (kitty @ launch --location first --keep-focus ~/.cargo/bin/scd)
kitty @ resize-window --match id:$id --axis reset
kitty @ resize-window --match id:$id -i -45
scd fish-init | source
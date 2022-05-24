let-env config = ($env.config | upsert hooks {
  env_change: {
    PWD: [{|before, after|
      ~/.cargo/target/release/scd cd $after
    }]
  }
})

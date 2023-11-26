#!/bin/sh 

start() {
  exec /home/ubuntu/mles-rs/mles/target/release/mles --history-limit=3000
}

stop() {
  exec sudo killall mles  
}

case $1 in
  start|stop) "$1" ;;
esac

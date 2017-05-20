#!/bin/sh 

start() {
  exec MLES_KEY=mles-devel /home/ubuntu/mles/mles-rs/mles/target/release/mles 
}

stop() {
  exec sudo killall mles  
}

case $1 in
  start|stop) "$1" ;;
esac

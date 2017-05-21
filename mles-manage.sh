#!/bin/sh 

start() {
  exec /home/ubuntu/mles/mles-rs/mles/target/release/mles 
}

stop() {
  exec sudo killall mles  
}

case $1 in
  start|stop) "$1" ;;
esac

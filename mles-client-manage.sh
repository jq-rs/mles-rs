#!/bin/sh 

start() {
  exec /home/ubuntu/mles/mles-rs/mles-client/target/release/mles-client 127.0.0.1 --use-websockets
}

stop() {
  exec sudo killall mles-client 
}

case $1 in
  start|stop) "$1" ;;
esac

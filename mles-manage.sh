#!/bin/sh
start() {
  exec /home/ubuntu/www/mles-rs/target/release/mles --domains mles.io --domains www.mles.io --cache . --wwwroot /home/ubuntu/www/mles-rs/static --limit 1000 --redirect
}

stop() {
  exec killall mles
}

case $1 in
  start|stop) "$1" ;;
esac

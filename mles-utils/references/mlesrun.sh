#!/bin/bash

for i in {1..300}
do
    ./target/release/mles-reference >> mles_tests.log
done

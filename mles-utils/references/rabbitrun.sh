#!/bin/bash

for i in {1..300}
do
    ./rabbitmqreference.py >> rabbit_tests.log
done

#!/usr/bin/env python
import pika
import sys 
import threading
import time

THREADCNT = 100

start = 0

def a_daemon():
    connection = pika.BlockingConnection(pika.ConnectionParameters(host='localhost'))
    channel = connection.channel()

    channel.exchange_declare(exchange='logs',
                             type='fanout')

    result = channel.queue_declare(exclusive='True')
    queue_name = result.method.queue

    channel.queue_bind(exchange='logs', queue=queue_name)

    def callback(ch, method, properties, body):
        if body == 'Hello World!':
            sys.exit()
        else:
            print("Invalid body!")

    channel.basic_consume(callback,
                          queue=queue_name,
                          no_ack=True)

    #print(' [*] Waiting for messages. To exit press CTRL+C')
    channel.start_consuming()

tlist = []

for i in range(THREADCNT):
    t =  threading.Thread(name='new', target=a_daemon)
    t.start()
    tlist.append(t)

time.sleep(2)
connection = pika.BlockingConnection(pika.ConnectionParameters(host='localhost'))
channel = connection.channel()

channel.exchange_declare(exchange='logs',
                             type='fanout')

start = time.time()

channel.basic_publish(exchange='logs',
                      routing_key='',
                       body='Hello World!')
#print(" [x] Sent 'Hello World!'")

for i in range(THREADCNT):
    tlist[i].join()

end = time.time()
print(end - start)

connection.close()

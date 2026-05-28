import sys, time, subprocess, http.client, traceback
import requests
import re
from os.path import exists
from threading import Thread
from queue import Queue, Empty
import pandas as pd

class MyThread(Thread):
	def __init__(self,id,queue,output):
		Thread.__init__(self)
		self.queue = queue
		self.id = id
		self.output = output

	def run(self):
		cnt=0
		try:
			while True:
				msg = queue.get_nowait()
				try:
					cnt=cnt+1		
					print('Thread'+str(self.id)+' ' +str(cnt)+' -->')
					tempoChiamata=time.time()
					r = self.transmit(host, port, path, '<?xml version="1.0" encoding="UTF-8"?>'+msg)
					print('Thread'+str(self.id)+' ' +str(cnt)+' <--')
					tempoChiamata=time.time()-tempoChiamata
					r =re.sub('</S:Envelope>','<tempoChiamata>'+str(tempoChiamata)+'</tempoChiamata></S:Envelope>',r)
					self.output.write(r+'\n')
					#time.sleep(0.5)
				except:
					print('Thread'+str(self.id)+" Errore per la chiamata "+msg)
					traceback.print_exc()
		except Empty:
			pass


	def transmit(self, host, port, path, message):
		url = 'http://' + str(host) + ':' + str(port) + str(path)
		headers = {'POST': path,
								'content-type': 'text/xml',
								'Host' : host + ":" + port,
								"User-Agent": "Python post",
								"Content-type" : "text/xml; charset=\"UTF-8\"",
								"Content-length": str(len(message)),
								"SOAPAction":"\"\""
			}
		body = message
		response = requests.post(url, data = body, headers = headers)
		if response.status_code != 200: print('ERROR')
		res = response.content.decode('latin1')
		return res



host = "192.168.39.18"
port="10480"
path = "/WPEService/wpeservice"
nThread = 1

inputFile = "richiesteProva.txt"
outputFile = "RisposteProva.txt"


requests_df = pd.read_csv('C:\\Users\\fontanesio\\Documents\\CLIENTI\\WIDIBA\\tmp\\testMassiva.txt', header=None)
requests_df.rename(columns={0 : 'requests'}, inplace=True)
lines = requests_df['requests'].tolist()
"""
f = open(inputFile,'r')
lines = f.readlines()
f.close()
"""
lines = requests_df['requests'].tolist()
queue = Queue()
for l in lines:
	if len(l.strip())>0:
		queue.put_nowait(l)

output=open('C:\\Users\\fontanesio\\Documents\\CLIENTI\\WIDIBA\\tmp\\' + outputFile,'w')

#print lines

t1=time.time()
threadlist = []
for i in range(nThread):
	t = MyThread(i,queue,output)
	t.start()
	threadlist.append(t)

for t in threadlist:
	t.join()

t1=time.time()-t1
#output.write('Tempo totale '+str(t1)+' sec\n')
#print "Tempo totale "+str(t1)+" sec"
#output.write(' '+str(len(lines)/t1)+' request per second')
#print ""+str(len(lines)/t1)+" request per second"


output.flush()
output.close()


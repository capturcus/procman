#!/usr/bin/env python3
import requests

base_url = "http://localhost:8080/api/v1/processes"
headers = {
    "Content-Type": "application/json"
}

response = requests.post(base_url, headers=headers, json={"cmd":"./test.sh"})
uuid = response.json()["uuid"]

with requests.get(base_url+"/"+uuid+"/live_log", headers=headers, stream=True) as response:
    response.raise_for_status()  # Raise an exception for HTTP errors
    for line in response.iter_lines():
        if line:
            decoded_line = line.decode('utf-8')
            print(decoded_line)

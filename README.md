# procman

A REST-based process manager.

- `GET /api/v1/processes` - get a list of processes (without logs)
- `GET /api/v1/processes/{uuid}` - get a specific process including all logs
- `POST /api/v1/processes` - start a process. The only argument required in the JSON body is `arg` - the command to run.
- `DELETE /api/v1/processes/{uuid}` - kill the process and delete it from the app
- `GET /api/v1/processes/{uuid}/live_log` - get a streaming live log from the process in real time

The `live_log.py` script uses `requests` to execute a POST and a GET to the live log endpoint. There is also a test script `test.sh` that generates logs every second.

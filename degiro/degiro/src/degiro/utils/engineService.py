import requests
import paramiko
import pandas as pd


def execute(host, port, path, message):
    """Execute a SOAP request."""
    url = 'http://' + str(host) + ':' + str(port) + str(path)
    headers = {
        'POST': path,
        'content-type': 'text/xml',
        'Host': host + ":" + port,
        "User-Agent": "Python post",
        "Content-type": "text/xml; charset=\"UTF-8\"",
        "Content-length": str(len(message)),
        "SOAPAction": "\"\""
    }

    body = message
    response = requests.post(url, data=body, headers=headers)

    if response.status_code != 200:
        print('ERROR')
    res = response.content.decode('latin1')
    return res


def esegui_massiva(request_list, endpoint):
    """
    :param request_list: list of request messages
    :param endpoint: dict, keys must be host, port, path
    :return: list of responses
    """
    responses = []
    i = 1
    for request in request_list:
        responses.append(
            execute(
                host=endpoint['host'],
                port=endpoint['port'],
                path=endpoint['path'],
                message=request)
        )
        print(f"\r{i}", end="")
        i += 1
    return responses


def singleRequest(host, port, path, request):
    r = execute(host, port, path, '<?xml version="1.0" encoding="UTF-8"?>' + request)
    response = str(r)
    return response


def get_server_connection(host, user_name, password):
    """
    :param host: str server IP
    :param user_name: str username
    :param password: str password
    :return: paramiko.SSHClient or None if connection failed
    """
    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())

    assert isinstance(user_name, str), 'inserire user_name'
    assert isinstance(password, str), 'inserire password'

    try:
        ssh.connect(host, username=user_name, password=password)
        return ssh
    except Exception as e:
        print(f"Connection failed: {e}")
        return None


def download_file(local_path, remote_path, server_connection):
    """
    :param local_path: str local directory with file name
    :param remote_path: str remote directory with file name
    :param server_connection: dict, keys must be HOST, USER, PASSWORD
    """
    ssh_conn = get_server_connection(
        host=server_connection['HOST'],
        user_name=server_connection['USER'],
        password=server_connection['PASSWORD']
    )
    if ssh_conn is None:
        raise ConnectionError('Connection failed')
    try:
        sftp = ssh_conn.open_sftp()
        sftp.get(remote_path, local_path)
        sftp.close()
        ssh_conn.close()
        print(f'Done : {remote_path} -> {local_path}')
        return None
    except Exception as e:
        print(f'Download failed - {e}')
        ssh_conn.close()
        return None


def upload_file(local_path, remote_path, server_connection):
    """
    :param local_path: str local directory with file name
    :param remote_path: str remote directory with file name
    :param server_connection: dict, keys must be HOST, USER, PASSWORD
    """
    ssh_conn = get_server_connection(
        host=server_connection['HOST'],
        user_name=server_connection['USER'],
        password=server_connection['PASSWORD']
    )
    if ssh_conn is None:
        raise ConnectionError('Connection failed')
    try:
        sftp = ssh_conn.open_sftp()
        sftp.put(local_path, remote_path)
        sftp.close()
        ssh_conn.close()
        print(f'Done : {local_path} -> {remote_path}')
        return None
    except Exception as e:
        print(f'Upload failed - {e}')
        ssh_conn.close()
        return None

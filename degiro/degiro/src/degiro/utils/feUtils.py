import xml.etree.ElementTree as et
import pandas as pd
import re, os
import requests
import paramiko
from pathlib import Path

def load_xml(file_path: Path):        
    with open(file_path) as mock:
        tree = et.parse(mock)
        return tree.getroot()

def export_xml(xml, path_out: Path):
    from xml.dom import minidom

    xml_str = minidom.parseString(et.tostring(xml)).toprettyxml(indent="   ")
    with open(os.path.join(path_out), "w") as f:
        f.write(xml_str)

def dump_xml(ndg: str, idChiamata: str, df: pd.DataFrame, column: str, path: Path):
    df.loc[(ndg, idChiamata), [column]].to_csv(path, index=False, header=False)

def get_regex_in_sqrd_bkt(mess: str):
    if re.search(r'(?<=\[)(.*?)(?=\])', str(mess)):
        return re.search(r'(?<=\[)(.*?)(?=\])', str(mess)).group(1)
    else:
        return None

def remove_tag(tag_name: str, xml):
    for parent in xml.findall(f'.//{tag_name}/..'):
        # Find each tag_name element
        for element in parent.findall(tag_name):
            # Remove the tag_name element from its parent element
            parent.remove(element)
    return xml

"""
    Parsing input
"""
def get_ndg(xml_as_str: str):
    return et.fromstring(xml_as_str).find('.//ndg').text

def get_idChiamata(xml_as_str: str):
    return et.fromstring(xml_as_str).find('.//arg0/idChiamata').text

def get_idChiamante(xml_as_str: str):
    return et.fromstring(xml_as_str).find('.//arg0/idChiamante').text

def get_servizio(xml_as_str: str):
    try:
        return et.fromstring(xml_as_str).find('.//servizio').text 
    except:
        return None

def get_strategia(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: str, tag strategia
    """    
    xml = et.fromstring(xml_as_str)
    return xml.find('.//parametriPicking/codiceStrategia').text

def get_importo_investibile(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: str, tag importo investibile
    """    
    xml = et.fromstring(xml_as_str)
    return float(xml.find('.//importoInvestibile').text)

def get_ctv_mantengo(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: float, ctv mantengo
    """    
    xml = et.fromstring(xml_as_str)
    ctv_mantengo = 0.0
    for saldo in xml.find('.//portafoglio').findall('.//saldi'):
        ctv_mantengo += float(saldo.find('.//ctv').text)
        if float(saldo.find('.//ctvDelta').text) != 0.0:
            ctv_mantengo += float(saldo.find('.//ctvDelta').text)
    return ctv_mantengo

def get_size_libera(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: float, investibile libero
    """
    return get_importo_investibile(xml_as_str) - get_ctv_mantengo(xml_as_str)

def get_espcon(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    return xml.find('.//esperienza').text

def get_cod_rischio(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    return xml.find('.//codiceProfilo').text

def get_numerosita_mantengo(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    numero_saldi_delta = 0
    for saldo in xml.find('.//portafoglio').findall('.//saldi'):
        if float(saldo.find('.//ctvDelta').text) == 0.0:
            numero_saldi_delta += 1
    return numero_saldi_delta

def get_numerosita_delta(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: int, numerosita delta <> 0.0
    """    
    xml = et.fromstring(xml_as_str)
    numero_saldi_delta = 0
    for saldo in xml.find('.//portafoglio').findall('.//saldi'):
        if float(saldo.find('.//ctvDelta').text) != 0.0:
            numero_saldi_delta += 1
    return numero_saldi_delta

def get_movimentazioni_delta(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    saldi_delta = {}
    for saldo in xml.find('.//portafoglio').findall('.//saldi'):
        if float(saldo.find('.//ctvDelta').text) != 0.0:
            if saldo.find('.//codRischio').text not in saldi_delta.keys():
                saldi_delta.update({
                    saldo.find('.//codRischio').text : {
                        'ctvDelta' : float(saldo.find('.//ctvDelta').text),
                        'ctv' : float(saldo.find('.//ctv').text)
                    }
                })
            else:
                saldi_delta.update({
                    saldo.find('.//codRischio').text+'_2' : {
                        'ctvDelta' : float(saldo.find('.//ctvDelta').text),
                        'ctv' : float(saldo.find('.//ctv').text)
                    }
                })
    return saldi_delta



"""
    Parsing output
"""
def get_esito_316(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    if xml.find('.//return/esito').text == 'OK':
        return xml.find('.//return/risultatoAdeguatezza/esito').text
    else:
        return xml.find('.//return/returnMessage').text

def get_esito_mifid(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: esiti, dict, keys codice controllo mifid, values esito
    """
    xml = et.fromstring(xml_as_str)
    esiti = {}
    for controllo in xml.findall('.//resultControlli'):
        esiti.update({
            controllo.find('.//codice').text : controllo.find('.//esito').text
        })
    return esiti

def get_payload_costi_benefici(analisi_benefici: dict):
    """
    Spacchetto payload analisiBenefici
    :param x: dict, payload analisiBenefici
    :return: dict
    """
    d = {}
    try:
        d.update({
            'qualitaOld_global': analisi_benefici['qualitaOld'],
            'qualitaNew_global': analisi_benefici['qualitaNew'],
            'beneficio': analisi_benefici['beneficio']
        })
        for beneficio in analisi_benefici['dettaglioBenefici']:
            d.update({beneficio['codice'] + '_qualitaOld': beneficio['qualitaOld']})
            d.update({beneficio['codice'] + '_qualitaNew': beneficio['qualitaNew']})
        return d

    except Exception as e:
        print(e)
        return d

def is_response_ok(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: bool, 0 : esito KO, 1 esito OK
    """    
    xml = et.fromstring(xml_as_str)
    if xml.find('.//esito').text == 'KO':
        return 0
    elif xml.find('.//esito').text == 'OK':
        return 1

def get_return_message(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: str, esito, None as parachute
    """
    xml = et.fromstring(xml_as_str)
    if xml.find('.//esito').text == 'KO':
        return xml.find('.//returnMessage').text
    else:
        return "OK"

def get_return_message_mod(xml_as_str: str):
    """
    :param: xml_as_str, str, must be an xml 
    :return: str, esito, None as parachute
    """
    xml = et.fromstring(xml_as_str)
    if xml.find('.//esito').text == 'KO':
        return xml.find('.//returnMessage').text
    else:
        return None

def get_error_code(mess: str):
    """
    :param: mess, str
    :return: str
    """    
    if mess != "OK":
        return re.search(r'(?<=\()(.*?)(?=\))', str(mess)).group(1)
    else:
        return "OK"

def get_error_code_mod(mess: str):
    """
    :param: mess, str
    :return: str
    """
    if mess:
        return re.search(r'(?<=\()(.*?)(?=\))', str(mess)).group(1)
    else:
        return 'OK'

def get_cash_non_allocato(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    if xml.find('.//cashNonAllocato').text:
        return float(xml.find('.//cashNonAllocato').text)
    else:
        return None

def get_aderenza(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    if xml.find('.//return/esito').text == 'OK':
        return float(xml.find('.//return/statistiche/aderenza').text)
    else:
        return -1

def get_numerosita_delta_51(xml_as_str: str):
    xml = et.fromstring(xml_as_str)
    numero_saldi_delta = 0
    if xml.find('.//return/esito').text == 'KO':
        return numero_saldi_delta
    elif xml.find('.//return/esito').text == 'OK':
        for saldo in xml.find('.//ptfOut').findall('.//saldi'):
            try:
                # questa riga serve solo per creare l ecezione in caso manchi l interno
                _ = saldo.find('.//codInterno').text
            except:
                numero_saldi_delta += 1
    return numero_saldi_delta


"""
    Massive
"""
def get_lista_prodotti(nome_lista: str, param_ppe_path: Path):
    """
    """
    param_ppe = load_xml(param_ppe_path)

    for lista in param_ppe.findall('.//listeTitoli/listaTitoli'):
        if lista.find('.//codiceListaTitoli').text == nome_lista:
            l = [isin.text for isin in lista.findall('.//listaItem/valore')]
    return l

def get_info_req_516(df: pd.DataFrame, field_req: str):
    df['size_libera'] = df[field_req].apply(get_size_libera)
    df['is_ok_picking'] = df[field_req].apply(is_response_ok)
    df['returnMessage'] = df[field_req].apply(get_return_message)
    df['errorCode'] = df.returnMessage.apply(get_error_code)
    df['strategia_req_516'] = df[field_req].apply(get_strategia)
    df['importo_investibile'] = df[field_req].apply(get_importo_investibile)
    df['espcon'] = df[field_req].apply(get_espcon)
    df['profilo_rischio'] = df[field_req].apply(get_cod_rischio)
    df['numerosita_mantengo'] = df[field_req].apply(get_numerosita_mantengo)
    df['numerosita_delta'] = df[field_req].apply(get_numerosita_delta)
    df['mov_delta'] = df[field_req].apply(get_movimentazioni_delta)
    df['servizio'] = df[field_req].apply(get_servizio)
    df['cash_non_alloc'] = df[field_req].apply(get_cash_non_allocato)

def esegui_massiva_from_df(requests: pd.DataFrame, field: str, endpoint: dict, new_field='Responses'):
    """
    :param: requests, pd.DataFame
    :param: field, str (column of requests)
    :param: endpoint, dict, keys must be host, port, path
    :param: new_field, str name of new column containing the responses. Default Responses
    :return: pd.Dataframe
    """
    assert field in requests.columns, 'field parameter must be contained into requests dataframe columns'
    
    responses = []
    i = 1
    for request in requests[field]:
        responses.append(
            execute(
                host=endpoint['host'], 
                port=endpoint['port'], 
                path=endpoint['path'], 
                message=request)
            )
        print(f"\r{i}", end="")
        i += 1
    requests[new_field] = responses

def esegui_massiva_from_list(requests: list, endpoint: dict):
    """
    :param: requests, list
    :param: endpoint, dict, keys must be host, port, path
    :return: list
    """
    responses = []
    i = 1
    for request in requests:
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

def get_info_esito_massiva(df: pd.DataFrame, field: str):
    """
    """
    return pd.DataFrame(
        data={
            "numerosita_esito" : df[field].apply(get_return_message).apply(get_error_code).value_counts(), 
            "perc_esito" : df[field].apply(get_return_message).apply(get_error_code).value_counts()/df[field].apply(get_return_message).apply(get_error_code).value_counts().sum()
            })

"""
    Utils
"""
def execute(host: str, port: str, path: str, message: str):
    """
    """
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

def execute_single_request(host: str, port: str, path: str, request: str):
    r = execute(host, port, path, '<?xml version="1.0" encoding="UTF-8"?>' + request)
    response = str(r)
    
    return response

def get_server_connection(host: str, user_name: str, password: str):
    """
    :param: host, str server IP
    :param: user_name, str username
    :param: password, str password
    :return: object paramiko.SSHClient() (server connection) or None if connection failed
    """
    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    
    assert isinstance(user_name, str), 'inserire user_name'
    assert isinstance(password, str), 'inserire password'
    
    try:
        ssh.connect(host, username=user_name, password=password)
        return ssh

    except:
        print("Connection failed")
        return 200

def download_file(local_path: Path, remote_path: Path, server_connection):
    """
    :param: local_path, str local directory with file name
    :param: remote_path str remote directory with file name
    :param: server_connection dict, keys must by HOST USER and PASSWORD
    """
    ssh_conn = get_server_connection(host=server_connection['HOST'], user_name=server_connection['USER'], password=server_connection['PASSWORD'])
    assert ssh_conn != 200, 'Connection failed'
    try:
        sftp = ssh_conn.open_sftp()
        sftp.get(remote_path, local_path)
        sftp.close()
        ssh_conn.close()
        print(f'Done : {remote_path} -> {local_path}')
        return None
    except Exception as e:
        print(f'Download failed - {e}')

def upload_file(local_path, remote_path, server_connection):
    """
    :param: local_path, str local directory with file name
    :param: remote_path str remote directory with file name
    :param: server_connection dict, keys must by HOST USER and PASSWORD
    """
    ssh_conn = get_server_connection(host=server_connection['HOST'], user_name=server_connection['USER'], password=server_connection['PASSWORD'])
    assert ssh_conn != 200, 'Connection failed'
    try:
        sftp = ssh_conn.open_sftp()
        sftp.put(local_path, remote_path)
        sftp.close()
        ssh_conn.close()
        print(f'Done : {local_path} -> {remote_path}')
        return None
    except Exception as e:
        print(f'Upload failed - {e}')

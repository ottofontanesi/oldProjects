import logging.config
import yaml
import os
import locale
import json
from pathlib import Path

__module_path__ = Path(os.path.dirname(os.path.realpath(__file__)))


def load_config(path):
    with open(__module_path__ / Path(path), "r") as f:
        return yaml.safe_load(f)


CONFIG = load_config("config/config.yml")
LOG_CONFIG = load_config("config/config_log.yml")
NRT_FOLDER = json.load(open(__module_path__ / Path("config/nrt.json")))
SERVER_IP = json.load(open(__module_path__ / Path("config/server_ip.json")))
ENDPOINTS = json.load(open(__module_path__ / Path("config/endpoints.json")))

logging.config.dictConfig(LOG_CONFIG)
logger = logging.getLogger("DEGIRO")

locale.setlocale(locale.LC_ALL, CONFIG['locale'])

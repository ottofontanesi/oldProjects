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


def load_json(path):
    with open(__module_path__ / Path(path), "r") as f:
        return json.load(f)


CONFIG = load_config("config/config.yml")
LOG_CONFIG = load_config("config/config_log.yml")
NRT_FOLDER = load_json("config/nrt.json")
SERVER_IP = load_json("config/server_ip.json")
ENDPOINTS = load_json("config/endpoints.json")

logging.config.dictConfig(LOG_CONFIG)
logger = logging.getLogger("DEGIRO")

locale.setlocale(locale.LC_ALL, CONFIG['locale'])

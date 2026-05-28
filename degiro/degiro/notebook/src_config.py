import sys
import os


def append_path():
    SRC_PATH = os.path.abspath("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\src\\")
    if SRC_PATH not in sys.path:
        sys.path.append(SRC_PATH)

append_path()
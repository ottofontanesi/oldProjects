from setuptools import setup, find_packages
#from pip._internal.req import parse_requirements

#install_reqs = parse_requirements("./requirements.txt", session=False)
#reqs = [str(ir.req) for ir in install_reqs]


setup(name='degiro',
      version='0.0.1',
      description='laboratorio per progetti interni',
      author='Otto Fontanesi',
      author_email='otto.fonta@hotmail.com',
      packages=find_packages(where='src'),
      package_dir={'': 'src'},
      setup_requires=[],
      install_requires=[],
      package_data={
          '': []}
      )

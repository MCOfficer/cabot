import os
import pathlib
from shutil import copy2
import subprocess

from behave import *

from functionals.fixtures import wsgi

def run_command(context):
    def run_command_impl(command):
        cmd = []
        in_param = True
        for part in command.split():
            if in_param:
                if part.startswith('\''):
                    cmd.append(part[1:])
                    in_param = False
                else:
                    cmd.append(part)
            else:
                if part.endswith('\''):
                    in_param = True
                    cmd[-1] += ' ' + part[:-1]
                else:
                    cmd[-1] += ' ' + part
        return subprocess.run(
            cmd,
            capture_output=True,
            text=True,
        )
    return run_command_impl


def before_all(context):

    test_dir = pathlib.Path(__file__).resolve().parent.parent
    test_dir.joinpath('cabot').unlink(missing_ok=True)
    working_dir = test_dir.parent.parent
    os.chdir(working_dir)
    subprocess.run(['cargo', 'build', '--features', 'functional_tests'])
    copy2(
        working_dir.joinpath('target', 'debug', 'cabot'),
        test_dir,
    )
    os.chdir(test_dir)
    os.environ['PATH'] += os.pathsep + str(test_dir)

    wsgi.setUp()



def before_scenario(context, scenario):
    context.stash = {}
    context.run = run_command(context)


def after_all(context):
    wsgi.tearDown()
    pathlib.Path('outfile.tmp').unlink(missing_ok=True)

"""Temporary CI-only capture of real UI motion; removed from the final PR diff."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

label = sys.argv[1]
folder = Path('evidence') / label
folder.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, XDG_CONFIG_HOME=f'/tmp/hinge-config-{label}', LIBGL_ALWAYS_SOFTWARE='1', WGPU_BACKEND='gl')
log = open(folder / 'app.log', 'w')
app = subprocess.Popen(['target/ci/solidcraft', '--control', '7880'], env=env, stdout=log, stderr=log)

def call(method, params=None):
    with socket.create_connection(('127.0.0.1', 7880), timeout=30) as conn:
        conn.sendall((json.dumps({'id':1,'method':method,'params':params or {}})+'\n').encode())
        response = json.loads(conn.makefile('rb').readline())
        if not response.get('ok'):
            raise RuntimeError(response)
        return response.get('result')

def command(name, **params):
    return call('engine.execute', {'command':name,'params':params,'animate':False})

def record(kind):
    states = []
    for i in range(32):
        time.sleep(0.10)
        states.append(call('ui.inspect'))
        call('ui.screenshot', {'path':str((folder / f'{kind}{i:03d}.png').resolve())})
    (folder / f'{kind}-states.json').write_text(json.dumps(states, indent=2))
    subprocess.run(['ffmpeg','-y','-framerate','10','-i',str(folder/f'{kind}%03d.png'),
        '-vf','scale=trunc(iw/2)*2:trunc(ih/2)*2','-c:v','libx264','-pix_fmt','yuv420p',str(folder/f'{kind}.mp4')],check=True)

try:
    for _ in range(120):
        if app.poll() is not None:
            raise RuntimeError((folder / 'app.log').read_text())
        try:
            call('ui.inspect')
            break
        except (OSError, ValueError):
            time.sleep(0.5)
    call('ui.agent_cursor', {'show':False})
    command('file.new', name='Hinge preview reproduction')
    command('component.create', name='Base')
    command('solid.box', length=120, width=80, height=6, body_name='Base')
    # Enough separate keys to exercise the asynchronous preview on a normal
    # multi-body laptop, rather than a two-box model whose worker finishes instantly.
    for row in range(5):
        for column in range(14):
            command('solid.box', length=6, width=5, height=0.8,
                    corner=[5+8*column,35+8*row,6.1], body_name=f'Key {row}-{column}')
    command('component.activate', component='root')
    command('component.create', name='Lid')
    command('solid.box', length=120, width=80, height=4, corner=[0,0,10], body_name='Lid')
    command('component.activate', component='root')
    command('occurrence.ground', occurrence='Base:1', grounded=True)
    command('joint.origin', name='Pivot', occurrence='Base:1', point=[60,80,8], z=[1,0,0])
    command('joint.as_built', type='revolute', a='Base:1', b='Lid:1', at='Pivot')
    command('joint.limits', joint=1, min=-135, max=0)
    command('joint.drive', joint=1, value=-60)
    command('view.apply', view='iso')
    call('ui.menu', {'x':200,'y':250,'target':{'type':'joint','id':1}})
    time.sleep(0.2)
    call('ui.menuPick', {'item':'Animate Joint'})
    record('animate')
    call('ui.key', {'key':'Escape'})
    call('ui.start', {'command':'joint.drive'})
    time.sleep(0.4)
    handles = call('ui.handles', {'prefix':'drive_slider:'})
    handle = next(h for h in handles if h['name']=='drive_slider:0')
    x,y = handle['x'],handle['y']
    call('ui.drag', {'x0':x-65,'y0':y,'x1':x+65,'y1':y,'steps':200,'hold_end':150})
    record('slider')
    call('ui.key', {'key':'Escape'})
finally:
    app.terminate()
    try:
        app.wait(timeout=5)
    except subprocess.TimeoutExpired:
        app.kill()
    log.close()

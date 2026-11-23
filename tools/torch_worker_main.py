"""MTG Zero's torch worker entry (docs/ENGINE.md 244; zero-core docs/az.md 80): shipped as `worker_main.py` at the root of
a torch worker build (tools/farm_build.py --torch), next to the commit's `mtgz/` (with its `_engine.pyd`) and zero-core's
`third_party/zero-core/python/zero/` (a checkout's layout: `mtgz` finds its pinned core there). zero-farm's `zf-pyshim.exe` runs it with the torch runtime pack's Python:

    <runtime>/python/python.exe -I -X utf8 -u <build>/worker_main.py serve

(`-I`: nothing from the PC's own Python, its environment or the script's directory reaches the import path but what this
file puts there). It speaks aw/1 through `zero.farm_torch_worker`; `hello` goes out before torch is imported.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def is_build():
    """This file at a build's root (next to mtgz/), not in a checkout's tools/."""
    return os.path.isdir(os.path.join(HERE, 'mtgz'))


def paths():
    """The import roots: a build's (this file at its root), else a checkout's (this file in tools/: `python
    tools/torch_worker_main.py serve` plays from the checkout, its tests and gate); both lay zero-core out as a checkout."""
    repo = HERE if is_build() else os.path.dirname(HERE)
    return HERE, [repo, os.path.join(repo, 'third_party', 'zero-core', 'python')]


def pycache():
    """Bytecode goes outside the build and the runtime pack (their files are exactly the signed ones, verified before
    every start): `ZEROFARM_PYCACHE_DIR` when the agent sets one, else <temp>/zero-farm-pycache - kept between workers,
    so torch's ~2,000 modules compile once per machine, not at every start."""
    import tempfile
    sys.pycache_prefix = os.environ.get('ZEROFARM_PYCACHE_DIR') or os.path.join(tempfile.gettempdir(), 'zero-farm-pycache')
    # (zf-pyshim runs python -B: nothing compiled is written at all. torch's own caches - extensions, kernels, hub - go
    # to the work directory, never next to the signed files)
    cache = os.path.join(os.getcwd(), '.torch-cache')
    for k in ('TORCH_HOME', 'TORCH_EXTENSIONS_DIR', 'TORCHINDUCTOR_CACHE_DIR', 'TRITON_CACHE_DIR', 'XDG_CACHE_HOME'):
        os.environ.setdefault(k, cache)


def main():
    build, roots = paths()
    if is_build():
        pycache()
    sys.path[:0] = roots
    if len(sys.argv) > 1 and sys.argv[1] == 'serve':
        # the protocol's stdout first: nothing an import prints may reach it
        from zero import farm_torch_worker as W
        out = W.Out(W.protocol_stdout())
        bid = W.build_id(build)
        out.event('hello', **W.hello(bid, 'mtg'))
        import mtgz.az  # noqa: F401  (the game package: its games, layouts and settings; no torch)
        return W.serve(build, 'mtg', out=out, hello_sent=True)
    from zero import farm_torch_worker as W
    import mtgz.az  # noqa: F401
    return W.main(sys.argv[1:], build, 'mtg')


if __name__ == '__main__':
    sys.exit(main())

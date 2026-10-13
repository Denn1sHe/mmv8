#!/usr/bin/env bash
# check-sdk-python-env.sh —— sdk-python 扩展面自检（防「幽灵 .so」）
#
# 背景（陈旧陷阱第 3/4 例，已给机制）：
#   `uv run`（默认同步口径）会用 **uv 缓存中的 wheel** 覆盖 `maturin develop` 的成果 ——
#   `site-packages/iv8/iv8*.so` 会被打回旧构建（连 mtime 一并还原），API 表面凭空缺失，
#   而「时间戳 / 文件大小 / 构建 EXIT=0」**全都看不出来**。
#
# 本 crate 的**唯一正确 recipe**：
#   cd crates/sdk-python
#   uv run --active --with maturin maturin develop      # 重建并就地安装
#   uv run --no-sync pytest                             # 跑测试（禁止复用同步口径）
# 注意：**plain `uv run pytest` 对本 crate 禁用**（会覆盖扩展）。
#
# 判定口径：`hasattr(iv8.Context, '<新 API>')` **实证** —— 不以时间戳/大小/EXIT=0 为据。

set -euo pipefail

cd "$(dirname "$0")/../crates/sdk-python"

echo "[check-sdk-python-env] 1/3 扩展落地检验（hasattr 实证）…"
if ! uv run --no-sync python -c "
import iv8, sys
ok = hasattr(iv8.Context, 'page') and hasattr(iv8.Context, 'network')
print('  iv8.Context.page =', hasattr(iv8.Context, 'page'))
print('  iv8.Context.network =', hasattr(iv8.Context, 'network'))
sys.exit(0 if ok else 1)
"; then
  echo "[check-sdk-python-env] FAIL: iv8 扩展陈旧（缺 page/network）。修复："
  echo "  cd crates/sdk-python && uv run --active --with maturin maturin develop"
  exit 1
fi

echo "[check-sdk-python-env] 2/3 契约套件（--no-sync）…"
uv run --no-sync pytest -q

echo "[check-sdk-python-env] 3/3 OK"
echo "  recipe: maturin develop  ->  uv run --no-sync pytest"
echo "  警告：plain \`uv run pytest\` 对本 crate 禁用（会覆盖 maturin 产物）"

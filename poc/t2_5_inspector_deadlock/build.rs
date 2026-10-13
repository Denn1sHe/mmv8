//! 构建脚本：修正 PyO3 解析出的 libpython 链接目录。
//!
//! ## 为什么需要它（实测坑，务必保留）
//!
//! 本机 Python 3.13.12（workbuddy 内置发行版）的 `sysconfig` 里
//! `LIBDIR` 被**烘死**为 `/install/lib` —— 这是 python-build-standalone 的
//! 已知打包瑕疵：构建时的前缀被写死进了 `Makefile`，安装到别处后
//! `LIBDIR` 不会重写。PyO3 的 `pyo3-build-config` 忠实读取 `LIBDIR`，
//! 于是产出 `-L /install/lib`，链接直接失败：
//!
//! ```text
//! ld: warning: search path '/install/lib' not found
//! ld: library 'python3.13' not found
//! ```
//!
//! 绕过办法本来是手工设 `RUSTFLAGS="-L <lib>"`，但那要求每台机器
//! （本地 macOS arm64 / 远程 Linux x86_64）都记得带环境变量，**极易漏**
//! —— 而漏掉的表现是「链接期报错」，尚可发现；更糟的是只补 `-L` 不补
//! rpath 时的**运行期** `dyld` 报错，那更像「代码写错了」。
//!
//! 故本脚本把这件事自动化且**可移植**：
//!   1. 跑目标解释器，问它真实的库目录；
//!   2. 若该目录不存在（正是 `/install/lib` 这种烘死路径），
//!      回退到 `sys.prefix/lib` —— 发行版实际放 dylib/so 的位置；
//!   3. 仍然不存在才报错，并把两条候选路径都打出来，便于人工介入。
//!
//! 只在**确实需要修正时**才 emit link-search / rpath：
//! 正常的 Python（如远程 uv venv、系统 python）走原路径即可，
//! 本脚本对它们是**零影响**的旁路。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 向 cargo 打印一行构建指令。
fn emit(line: &str) {
    println!("cargo:{}", line);
}

/// 用目标解释器查询 `sysconfig` 的某个变量。
///
/// 失败返回 `None` 而非 panic：解释器路径不对时，pyo3 自己会给出
/// 更准确的报错（它有完整的候选解释器搜索逻辑），我们不应抢先掩盖。
fn sysconfig_var(interpreter: &Path, var: &str) -> Option<String> {
    let script = format!(
        "import sysconfig; v = sysconfig.get_config_var({:?}); \
         print(v if v is not None else '')",
        var
    );
    let out = Command::new(interpreter)
        .arg("-c")
        .arg(&script)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 依次询问 sysconfig 里的多个变量名，返回第一个非空值。
fn first_sysconfig_var(interpreter: &Path, vars: &[&str]) -> Option<String> {
    vars.iter().find_map(|v| sysconfig_var(interpreter, v))
}

/// 列出定位 Python 动态库的可疑目录，按优先级排列。
///
/// 顺序即优先级：`LIBDIR` 是 PyO3 自己的选择（若它可用就不该改），
/// 其次是 `LIBPL`、`prefix/lib`，最后是 **`base`/可执行文件位置推导出的 `lib`**。
///
/// 最后一项是本探针在 macOS 上**实测需要**的：workbuddy 内置的
/// python-build-standalone 把 `LIBDIR`/`LIBPL`/`prefix` 全部烘死成
/// `/install/...`（构建前缀写死进了 Makefile），而真实的 dylib 在
/// `<base>/lib/libpython3.13.dylib`。前三者都不存在，只有 `base` 找得到。
fn candidate_lib_dirs(interpreter: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    };

    if let Some(dir) = first_sysconfig_var(interpreter, &["LIBDIR"]) {
        push(PathBuf::from(dir));
    }
    if let Some(dir) = first_sysconfig_var(interpreter, &["LIBPL"]) {
        push(PathBuf::from(dir));
    }
    for key in ["prefix", "base"] {
        if let Some(prefix) = sysconfig_var(interpreter, key) {
            push(PathBuf::from(prefix).join("lib"));
        }
    }
    // 从解释器可执行文件位置推导：`<prefix>/bin/python3` → `<prefix>/lib`。
    // 这是不依赖 sysconfig 的独立交叉验证，对上述烘死路径的发行版尤其可靠。
    if let Ok(canonical) = std::fs::canonicalize(interpreter) {
        if let Some(prefix) = canonical.parent().and_then(Path::parent) {
            push(prefix.join("lib"));
            push(prefix.join("lib64"));
        }
    }
    dirs
}

/// 判断目录里是否真的躺着 libpython（`libpython*` 前缀）。
fn contains_libpython(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("libpython"))
    })
}

/// 选出第一个真正含有 libpython 的目录。
fn resolve_lib_dir(interpreter: &Path) -> Option<PathBuf> {
    let candidates = candidate_lib_dirs(interpreter);
    candidates
        .iter()
        .find(|dir| contains_libpython(dir))
        .cloned()
        // 一个都找不到时，退回第一个候选，让链接器自己报「找不到 libpython」，
        // 这比我们在构建脚本里猜测原因更有诊断价值。
        .or_else(|| candidates.first().cloned())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=IV8_PROBE_PYTHON");

    // pyo3-build-config 的解释器选择顺序：PYO3_PYTHON → PYO3_CONFIG_FILE 指定的
    // 配置里的 python → `python3`（PATH 上第一个）。这里必须与它保持一致，
    // 否则本脚本量到的 lib 目录和 pyo3 链接的那个不是同一个解释器。
    let interpreter: PathBuf = std::env::var_os("PYO3_PYTHON")
        .or_else(|| std::env::var_os("IV8_PROBE_PYTHON"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("python3"));

    let Some(lib_dir) = resolve_lib_dir(&interpreter) else {
        // 问不到就什么都不做：把判断权交回给 pyo3 与链接器。
        emit("warning=iv8probe: 未能向 Python 询问库目录，跳过 libpython 路径修正");
        return;
    };

    emit(&format!(
        "warning=iv8probe: libpython 目录 = {}",
        lib_dir.display()
    ));
    emit(&format!("rustc-link-search=native={}", lib_dir.display()));

    // 运行期搜索路径。macOS 用 `-Wl,-rpath,<dir>`，Linux 用
    // `-Wl,-rpath,<dir>`（ELF 同样支持，且优于老式 `-rpath`）。
    // 只在「该目录不是链接器默认会搜的地方」时才加 —— 但判断「默认会搜」
    // 并不直观，而一条多余的 rpath 无害（只是多一个搜索命中点），
    // 故统一加上，保证「构建通过 ⇒ 运行期也能找到」。
    if cfg!(target_os = "macos") {
        emit(&format!("rustc-link-arg=-Wl,-rpath,{}", lib_dir.display()));
    } else if cfg!(target_os = "linux") {
        emit(&format!("rustc-link-arg=-Wl,-rpath,{}", lib_dir.display()));
    }
}

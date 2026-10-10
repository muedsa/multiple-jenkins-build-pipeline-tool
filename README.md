# 多 Jenkins 发布管道工具

一个桌面小工具，把「按环境顺序逐个发布同一个服务」这件重复劳动自动化：

1. 在列表里勾选要发布的服务（常用的可以标成 ★，下次直接筛出来）；
2. 点「开始发布」，所有服务**并行**开始，各自沿环境链逐级推进；
3. 每个服务在某级环境构建 **成功** 后，自动在下一级触发 **同名 Job**；
4. 所有服务跑完后，弹出 **Windows 系统通知** 告诉你结果，文案可以自己改。

环境级数是**任意**的，想配几级配几级（如 `dev → sit → uat → prod`），顺序就是执行顺序。

技术栈：Tauri 2 + React 19 + TypeScript，后端 Rust，兼容 Jenkins **2.528.x** 的 REST API。

---

## 快速开始

```bash
# 安装依赖
pnpm install

# 开发模式（会自动拉起 Vite 并打开窗口）
pnpm tauri dev

# 打包安装程序
pnpm tauri build
```

要求本机已有 Rust 工具链（`cargo`）与 Node.js。

---

## 首次配置

打开应用后切到 **设置** 页，在「流水线环境」里按发布顺序逐个添加环境：

| 字段 | 说明 |
| --- | --- |
| 环境名 | 如 `dev` / `sit` / `prod`。**同时是凭据名**，见下方说明 |
| Jenkins 地址 | 例如 `http://jenkins-dev.corp.local:8080`，不要带 `/job/...` |
| 用户名 | Jenkins 登录账号 |
| API Token | 在 Jenkins 的 **个人设置 → Security → API Token** 里生成 |
| 轮询间隔 | 查构建状态的频率，默认 5 秒 |
| 单次构建超时 | 超过这个时间还没结束就判定失败，默认 90 分钟 |
| 忽略 TLS 证书校验 | 仅当内网用自签名证书时打开 |
| 本级失败后中止整条流水线 | 见下方「执行逻辑」 |

用 **↑ ↓** 调整顺序（顺序就是执行顺序），点标题栏展开编辑。每个环境填完点
**保存**，再点 **测试连接** 确认能通，顺便核对一下 Jenkins 版本。

### 环境名改名要注意

环境名就是 Windows 凭据管理器里的键。**给环境改名时，工具会自动把已保存的
Token 挪到新名字下**，所以正常改名不会导致 Token “丢失”。

如果新名字下本来就存了 Token，则保留已有的那份，不会覆盖。

### Token 存在哪里

**API Token 不会写进配置文件**，而是交给 Windows 凭据管理器保管
（在「控制面板 → 凭据管理器 → Windows 凭据」里能看到，条目名形如
`LegacyGeneric:target=dev.multiple-jenkins-build-pipeline-tool`）。

配置文件只存地址、用户名、服务清单和通知文案，位置是：

```
%APPDATA%\com.muedsa.multiple-jenkins-build-pipeline-tool\config.json
```

---

## 添加服务

回到 **发布** 页，有两种添加方式：

### 从 Jenkins 搜索添加（推荐）

点「**☁ 从 Jenkins 搜索**」，工具会调 dev 的 `api/json?tree=jobs[...]` 拉取
**所有可构建 Job**（含文件夹里的），在弹窗里直接搜索勾选、批量添加 ——
不用手输，也就不会把 Job 名写错。

- 列表里带文件夹层级的 Job 会以灰色小字补出完整路径（如 `team-a/svc-1`），便于区分同名服务。
- 已经在清单里的会标「已添加」并置灰。
- 拉取一次后搜索都是本地过滤，不会每敲一个字就打一次 Jenkins。
- **文件夹、多分支流水线这类容器不会出现在列表里**，因为它们本身不能直接触发构建。

### 手动输入

点「**＋ 手动输入**」，可一次粘贴多行批量添加 —— 适合 Jenkins 一时连不上、
或想先按约定把服务名列好的场景。

### 通用说明

- **服务名就是 Jenkins Job 名**。工具假设 dev 和 test 上是同名 Job，只是服务器不同。
- 支持文件夹型 Job，用 `/` 分层即可，例如 `team-a/order-service`。
- 如果 Job 需要参数，在弹窗里加「参数名 / 参数值」。加了参数就走
  `buildWithParameters`；没加参数时先试 `build`，若 Job 其实定义了参数
  （Jenkins 会回 `400 Nothing is submitted`）会自动改走 `buildWithParameters` 用默认值。
- 点 ★ 标为常用，之后勾上「只看常用」就能快速筛出来。

---

## 执行逻辑

每个「服务 × 一条流水线」是一个**独立任务**，放进发布队列里：

- **入队即开跑，全部并行**：队列里的任务同时开始，互不等待。
- **每个任务内部逐级串行**：沿 `dev → sit → uat → prod` 依次推进，上一级成功才进入下一级。
- **每个任务独立结束**：某个服务失败或取消，**完全不影响**队列里其他任务。
- **单个取消**：卡片右上角的 `⊘` 只取消那一个任务；
  其他任务照常跑，队列也不清空。
- **某一级失败的默认行为**：只把该级标为失败，**后面的环境继续发布**。
- **勾选「本级失败后中止整条流水线」**：该级一旦失败，它后面所有级都标记为「已跳过」。
- **只有 `SUCCESS` 算成功**：`UNSTABLE`（有测试失败）也按失败处理，避免带着问题往下走。
- **任务整体算成功**：它经过的每一级都成功才算成功；任何一级失败，卡片标红并列出原因。
- **重新入队**：同一个服务可以反复加入队列（每次都是新任务），方便重跑。
- **清理**：队列头部的「清理已结束」把已经跑完的任务移出列表，还在跑的不受影响。

### 通知

**每个任务跑完各自弹一次系统通知**，文案里带服务名和它自己的结果 ——
不会等到整批结束才提醒你。三种结果各有文案：

| 结果 | 默认标题 |
| --- | --- |
| 成功 | `✅ {{serviceName}} 发布完成` |
| 失败 | `❌ {{serviceName}} 发布失败` |
| 被取消 | `⊘ {{serviceName}} 已取消` |

已结束的任务卡片上有个 `↻` 可以单独重发它的通知。

### 托盘与退出

程序常驻托盘，**点窗口右上角的关闭按钮只会最小化到托盘，不会退出** ——
发布可能还在跑，误点关闭不该把程序杀掉。

| 操作 | 效果 |
| --- | --- |
| 点窗口关闭按钮 | 隐藏到托盘，程序继续在后台跑（发布不受影响） |
| 左键单击托盘图标 | 把窗口重新显示到前台 |
| 右键单击托盘图标 | 弹出菜单：显示主窗口 / 退出程序 |
| 菜单里选「退出程序」 | 真正退出 |

托盘图标悬停提示为「Jenkins 发布管道」。

### 通知文案变量

在设置页的文案里可以用这些占位符：

| 变量 | 含义 |
| --- | --- |
| `{{serviceName}}` | 本次通知对应的服务名（通知按服务发，这条最常用） |
| `{{reachedStage}}` | 实际推进到的最后一级环境 |
| `{{failedStage}}` | 出错/被取消的那一级环境 |
| `{{stageChain}}` | 环境链路，如 `dev → sit → prod` |
| `{{stageCount}}` | 环境个数 |
| `{{status}}` | 本任务结论：`成功` / `失败` / `已取消` |
| `{{duration}}` | 本任务耗时，例如 `3分12秒` |
| `{{finishedAt}}` | 完成时刻 |
| `{{serviceCount}}` / `{{successCount}}` / `{{failedCount}}` | 单任务通知里分别为 1 / 1或0 / 0或1 |
| `{{serviceList}}` / `{{successList}}` / `{{failedList}}` | 单任务通知里都是该服务名（或空） |

设置页里点变量标签就能插入，并且有实时预览。写错的变量会原样保留显示，
方便一眼看出名字拼错了，而不是被悄悄替换成空白。

---

## 它是怎么调 Jenkins 的

用的都是标准 REST 接口：

| 用途 | 请求 |
| --- | --- |
| 探测连通性 / 版本 | `GET /api/json?tree=mode,nodeDescription,version` |
| 列出可构建 Job | `GET /api/json?tree=jobs[name,fullName,url,_class]` |
| 取 CSRF crumb | `GET /crumbIssuer/api/json` |
| 触发构建 | `POST /job/{name}/buildWithParameters` 或 `POST /job/{name}/build` |
| 队列项查构建号 | `GET /queue/item/{id}/api/json?tree=executable[number,url],why,cancelled` |
| 查构建状态 | `GET /job/{name}/{n}/api/json?tree=number,building,result,duration,url,displayName` |
| 查最近一次构建 | `GET /job/{name}/lastBuild/api/json` |

几点实现上的说明：

- 认证用 **Basic Auth（用户名 + API Token）**。用 token 而不是密码，
  所以在 Jenkins 2.528.x 收紧密码认证之后依然可用。
- 触发构建前会先取一次 crumb 并带上，同时开启 cookie 支持，
  兼容开了 CSRF 保护的实例；没开这个保护的实例会自动跳过。
- 触发成功时 Jenkins 的 `Location` 头给的是**相对路径**（如 `/queue/item/12/`），
  客户端会补成绝对地址；末尾斜杠会去掉，否则拼出来是
  `/queue/item/12/?tree=...` 这种取不到数据的地址。
- 构建号必须等排队结束后从队列项里读，所以「排队中」和「执行中」是两个阶段。
- TLS 走系统自带的 Schannel（`native-tls`），不需要额外编译 OpenSSL。

---

## 开发

```
src/                      前端（React + TS）
  api.ts                  对 Tauri 命令的封装
  types.ts                与后端一一对应的类型
  App.tsx                 主界面、事件订阅、运行状态
  components/
    ServicePicker.tsx     服务选择（搜索 / 常用 / 批量添加）
    RunBoard.tsx          执行进度卡片与状态徽章
    ServiceModal.tsx      新增编辑服务（含构建参数）
    SettingsPage.tsx      服务器配置 + 通知文案
src-tauri/src/
  lib.rs                  Tauri 命令注册与全局状态
  config.rs               配置读写（JSON，不含 token）
  secret.rs               API Token 存取（系统凭据管理器）
  jenkins.rs              Jenkins REST 客户端
  pipeline.rs             并行编排 + 事件推送
  notify.rs               通知文案模板渲染 + 系统通知
  tray.rs                 系统托盘：关闭最小化、托盘菜单退出
src-tauri/tests/
  jenkins_integration.rs  对着内置假 Jenkins 跑的集成测试
tools/icons/              图标源文件（DSL + 各尺寸 PNG）与渲染脚本
```

跑测试：

```bash
cd src-tauri
cargo test
```

集成测试会自己起一个假 Jenkins，覆盖触发、排队、轮询、成功 / 失败 / 不稳定、
401、404、超时，以及相对 `Location` 补全等路径。

其中需要往 **真实** 系统凭据管理器写测试 token 的用例**默认跳过**
（不想在用户的凭据里留东西）。要连这些一起跑，**必须单线程**：

```bash
# PowerShell
$env:JENKINS_TOOL_KEYRING_TESTS = "1"; cargo test -- --test-threads=1
```

> 为什么必须单线程：Windows 凭据管理器的读写不是线程安全的，多个测试并发读写时
> 会偶发丢写（刚存进去马上读却读不到）。这是环境限制，不是被测代码的问题。
> 多线程下这些用例会自动跳过，不会误报失败。

测试用的条目名一律以 `inttest-` 开头，跑完会自行清理。

### 应用图标

图标源文件在 `tools/icons/`（Snapshot DSL + 各尺寸 PNG），设计说明见该目录下的 README。

`src-tauri/icons/` 里**每个尺寸都用手调的原图**，而不是从 512 主图自动缩放 ——
小尺寸下等比缩放会把箭杆缩进箭翼里，箭头糊成一团，所以 16 / 24 / 32 / 48px
是各自独立设计的。组装方式：

| ico 档位 | 来源 |
| --- | --- |
| 16 / 24 / 32 / 48 / 64 | `tools/icons/release-pipeline-<尺寸>.png` 原图 |
| 256 | 512 主图 LANCZOS 缩一半（Windows 用 PNG 压缩存这一档） |

`icon.png` 用 512 主图，`32x32.png` / `128x128.png` 用手调原图，
`128x128@2x.png` 与 Windows Store 系列由 512 主图缩放。

改了图标后重新生成：

```bash
# 1) 重新渲染各尺寸 PNG（需要 Snapshot 服务）
powershell -File tools/icons/build-all.ps1
# 2) 按上表重新组装 src-tauri/icons（含多尺寸 icon.ico）
```

图标经 `.rc` 编译进 exe，改完必须重新构建才会生效。Windows 还会缓存 exe 图标，
看不到变化时把 exe 挪个目录或重启资源管理器即可。

---

## 已知限制

- 判定标准是 Jenkins 的构建结果，不解析构建日志。
- 服务清单保存在本机配置文件里，不跟着仓库走。
- 「停止」只是不再推进后续环境，不会调 Jenkins 的 abort 接口中断正在跑的构建。

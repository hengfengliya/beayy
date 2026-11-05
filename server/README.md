# Beayy 服务端（自己部署）

Beayy 的同步服务和转写服务，可以在自己的电脑或服务器上用 Docker 跑。官方实例（sync.beayy.com）跑的就是这份代码。

| 镜像 | 做什么 | 资源 |
|---|---|---|
| `beayy-server` | 账号、设备之间同步（只存设备上加密好的数据块，服务器读不到内容）、转写任务的接口、可选的官方模型和地图代理 | 几十 MB 内存 |
| `beayy-asr`（可选） | 录音转文字、分说话人（FunASR：Paraformer＋标点、FSMN-VAD、CAM++），CPU 跑 | 4G 以上内存；镜像约 3G，模型第一次启动时再下载约 2G |

两个镜像挂同一个数据目录：`beayy-server` 收下录音放进 `data/jobs/`，`beayy-asr` 转完写回去。不起 `beayy-asr` 也能同步，只是录音一直排队。

## 快速开始

```bash
git clone https://github.com/hengfengliya/beayy && cd beayy/server
cp .env.example .env        # 至少填 BEAYY_INVITE（注册邀请码），或者 BEAYY_OPEN_SIGNUP=1 开放注册
docker compose up -d                    # 只起同步
docker compose --profile asr up -d      # 同步＋转写
curl http://127.0.0.1:8770/v2/health    # {"ok":true}
```

然后在 Beayy（电脑）里：

1. **设置 → 连接 → 同步服务器**，填这台服务的地址（要在登录前换；已登录的先在「账号」里退出）。
2. 先在 **设置 → 连接 → 同步方式** 里选「自建服务器」填上地址，再到 **设置 → 账号 → 注册**，邀请码填 `.env` 里的 `BEAYY_INVITE`（开放注册的不用填）。其他设备用同一个邮箱和主密码登录。
3. 要用转写或服务器上配的模型、地图：先给账号开通（见下面「开通账号」），再在 **设置 → 连接 → 转写服务** 填同一个地址。

手机（iPhone）在登录界面填同样的同步服务器地址，登录后转写、地图的配置从电脑同步过去。

## 部署在哪

### 本机（自己的电脑）

最省事，数据全在自己手上。限制：**手机在外面连不到**你家里的电脑，手表、手机的录音和位置只能回家后再传。另外 iPhone 只允许连 HTTPS 地址（局域网 IP 的 `http://` 连不上）。解决办法任选一个：

- **Tailscale**（推荐）：电脑和手机都装上、登录同一个账号，在电脑上
  ```bash
  tailscale serve --bg 8770
  ```
  得到 `https://<电脑名>.<tailnet>.ts.net` 这样的 HTTPS 地址，电脑和手机都填它。手机在外面用流量也能连。
- **内网穿透**：Cloudflare 隧道（见下面）、frp 这类工具把本机 8770 端口暴露成一个 HTTPS 域名。

只在电脑上用（不接手机）的话，填 `http://127.0.0.1:8770` 就行。

### VPS：Caddy 自动 HTTPS

一个解析到 VPS 的域名（如 `sync.example.com`），开放 80、443 端口：

```bash
# .env 里写 BEAYY_DOMAIN=sync.example.com
docker compose --profile caddy up -d                 # 加上 --profile asr 一起起转写
```

Caddy 自动申请和续期证书，地址就是 `https://sync.example.com`。

### VPS 或本机：Cloudflare 隧道

不用开端口、不用公网 IP。在 Cloudflare Zero Trust → Networks → Tunnels 建一条隧道，拿到令牌；Public Hostname 填你的域名，服务填 `http://beayy-server:8770`。

```bash
# .env 里写 CLOUDFLARE_TUNNEL_TOKEN=...
docker compose --profile tunnel up -d
```

注意 Cloudflare 免费版单个请求最长 100 秒、上传最大 100MB，Beayy 的接口都在这个范围内。

## 配置

全在 `.env`，说明见 [`.env.example`](.env.example)。常用的：

| 变量 | 做什么 |
|---|---|
| `BEAYY_INVITE` | 注册邀请码。不填、也没开放注册，就谁也注册不了 |
| `BEAYY_OPEN_SIGNUP` | `1`＝开放注册，不要邀请码。开放时建议一起设下面几项 |
| `BEAYY_USER_QUOTA_MB` | 每个账号最多存多少 MB（默认 0＝不限）。单个账号另设：`python admin.py storage <邮箱> <MB>`（0＝不限） |
| `BEAYY_MAX_USERS` | 最多开放多少个账号（默认 0＝不限），满了停止注册 |
| `BEAYY_NEW_USER_LLM_TOKENS` | 新注册的账号自动开通官方模型，每月这么多 token（默认 0＝不自动开通，用 `admin.py open`）；转写、地图不跟着开 |
| `BEAYY_NEW_USER_TRIAL_DAYS` | 自动开通的新账号试用几天（默认 0＝不限期）；到期后官方服务停用，要在设置里改用自己的模型；`admin.py trial` 单独延长 |
| `BEAYY_MIN_FREE_GB` | 硬盘剩余低于这么多 GB 停止注册（默认 20），低到一半连上传也停 |
| `BEAYY_SIGNUP_PER_IP`、`BEAYY_SIGNUP_PER_DAY` | 同一 IP 每天最多注册几个（默认 3）、每天一共最多几个（默认 50） |
| `BEAYY_TICKET_SECRET` | 转写、地图票据的签名密钥。不填自动生成，存在 `data/sync/ticket_secret` |
| `BEAYY_LLM_GATEWAY`、`BEAYY_LLM_KEY`、`BEAYY_LLM_MODEL` | 服务器替开通的账号调模型（OpenAI 兼容接口）。不填的话客户端用自己配的模型 |
| `BEAYY_AMAP_KEY`、`BEAYY_AMAP_JS_KEY`、`BEAYY_AMAP_JS_CODE` | 服务器替开通的账号调高德地图 |
| `BEAYY_HOTWORDS` | 转写热词：人名、项目名这类容易认错的，空格分隔 |
| `BEAYY_BIND` | `127.0.0.1`（默认，只给本机和反向代理）或 `0.0.0.0`（局域网、Tailscale 直连） |

改了 `.env` 以后 `docker compose up -d` 重建容器生效。

## 开通账号

转写、服务器上的模型和地图按账号开通（免得注册了就能用你的模型额度），每项可以设每月上限：

```bash
docker compose exec -u beayy beayy-server python admin.py open you@example.com              # 开通，不限量
docker compose exec -u beayy beayy-server python admin.py open you@example.com 0 3000 30000 # 模型不限、转写每月 3000 分钟、地图每月 3 万次
docker compose exec -u beayy beayy-server python admin.py list                               # 所有账号的开通情况和本月用量
docker compose exec -u beayy beayy-server python admin.py stop you@example.com              # 停用
```

只用同步、不用这几项的不用开通。

## 数据、备份、搬家

```
data/sync/beayy.db        账号、设备登录、数据块索引（SQLite）
data/sync/blobs/          数据块（密文），按 账号/设备/块名
data/sync/ticket_secret   自动生成的票据密钥（.env 没填时）
data/jobs/                转写任务（临时：音频转完就删，结果取走就删，最多 7 天）
data/models/              转写模型缓存
backup/                   每天一份 beayy.db，留 14 天
```

录音块转写完 7 天后由电脑删除，没转的 30 天后服务器删。搬家：停掉（`docker compose down`），整个目录（含 `.env`、`data/`、`backup/`）拷到新机器，再 `docker compose up -d`，客户端地址不变就不用改。

## 不用 Docker

Python 3.12：

```bash
pip install -r requirements.txt
BEAYY_INVITE=... uvicorn server:app --host 127.0.0.1 --port 8770    # 数据默认写在当前目录的 data/、backup/
# 转写另开一个进程（要 ffmpeg）：
pip install torch torchaudio --index-url https://download.pytorch.org/whl/cpu && pip install -r requirements-asr.txt
python worker.py
```

两个进程要用同样的 `BEAYY_SYNC_DIR`、`BEAYY_JOBS_DIR`。

## 接口

见 `server.py`（转写、地图）和 `syncapi.py`（账号、同步）开头的说明。和 Cloudflare 版（`../sync-server/`）接口相同。

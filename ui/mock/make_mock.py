#!/usr/bin/env python3
"""生成浏览器预览用的样例数据（全是虚构的，没有任何真实记录）。
用法：python3 app/ui/mock/make_mock.py [YYYY-MM-DD]  → 写出 mock/<命令名>.json 和 mock/mock.js（双击 index.html 用 file:// 打开时读它）
日期是「今天」，默认 2026-10-29（截图时把浏览器时钟拨到这天 16:20）；往前生成 39 天。
命令和返回格式见 app/src-tauri/COMMANDS.md。

样例是一个虚构的人：在公司做「会员系统改版」，晚上和周末写小说《雾港》，周二周四去健身房，周三下午在咖啡馆干活，
国庆去海边玩了三天。作息的量级（电脑 6～8 小时、手机 3.5～6 小时、拿起 50～100 次、每晚熬夜半小时到一个半小时、
每天 4～10 段停留）照着真人的数据定，内容、地名、人名全是编的。"""
import json
import random
import sys
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
TZ = timezone(timedelta(hours=8))
TODAY = date.fromisoformat(sys.argv[1]) if len(sys.argv) > 1 else date(2026, 10, 29)
NOW = datetime(TODAY.year, TODAY.month, TODAY.day, 16, 20, tzinfo=TZ)  # 今天只记到 16:20
FIRST = TODAY - timedelta(days=38)
HOLIDAY = {date(2026, 10, d) for d in range(1, 8)}  # 国庆
TRIP = {date(2026, 10, 2), date(2026, 10, 3), date(2026, 10, 4)}  # 去海边
WD = "一二三四五六日"

MAC, WIN, PHONE = "mac-Demo-MacBook-Pro", "win-DESKTOP-DEMO01", "iphone-1A2B3C4D"


def at(d, h, m=0, s=0):
    return datetime(d.year, d.month, d.day, tzinfo=TZ) + timedelta(hours=h, minutes=m, seconds=s)


def iso(t):
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def hm_(t):
    return t.astimezone(TZ).strftime("%H:%M")


def dur(sec):
    return f"{sec / 3600:.1f} 小时" if sec >= 3600 else f"{round(sec / 60)} 分钟"


# ---------- 电脑上做的事：(标题, 描述, 类型, 领域, 项目, 经过, 涉及, app) ----------

A_CLAUDE, A_CHROME, A_CODE, A_LARK, A_WX, A_OBS, A_TERM, A_MUSIC = (
    "com.anthropic.claudefordesktop", "com.google.Chrome", "com.microsoft.VSCode", "com.electron.lark",
    "com.tencent.xinWeChat", "md.obsidian", "com.apple.Terminal", "com.netease.163music")
MAC_APP_NAMES = {A_CLAUDE: "Claude", A_CHROME: "Google Chrome", A_CODE: "Visual Studio Code", A_LARK: "飞书",
                 A_WX: "微信", A_OBS: "Obsidian", A_TERM: "终端", A_MUSIC: "网易云音乐"}

PHASES = [  # 会员系统改版每个阶段做的事（日期起点, 任务）
    (date(2026, 9, 21), [
        ("会员系统需求评审", "你和产品过了会员系统改版的需求文档，标出积分规则里三处说不清的地方。", "方案讨论",
         ["逐条过了会员等级、积分、权益三块需求，积分过期规则前后矛盾。", "约好周四前由产品补上过期规则的例子。"], ["会员系统改版：需求文档第二版，积分过期规则待补"], [A_LARK, A_CHROME, A_CLAUDE]),
        ("梳理旧会员接口", "你把旧会员服务的 23 个接口列成表，标出哪些能直接复用。", "写作",
         ["按读、写分组列出旧接口，9 个能直接复用，6 个要拆。", "顺手记下两个早就没人调用的接口，准备下线。"], ["会员系统改版：旧接口清单"], [A_CODE, A_CHROME, A_OBS]),
        ("和 Claude 讨论表结构", "你在 Claude 里比了两种积分流水表的设计，选了按月分表。", "方案讨论",
         ["比了单表加索引和按月分表两种做法，估了三年的数据量。", "选按月分表，查询多一层路由，但老数据好归档。"], ["会员系统改版：积分流水表设计"], [A_CLAUDE, A_CODE]),
    ]),
    (date(2026, 10, 8), [
        ("写积分服务", "你写完积分发放和扣减两个接口，单测过了 18 个。", "编码",
         ["实现积分发放、扣减，扣减时先锁账户再写流水。", "补了并发扣减的单测，跑出一个余额变负的问题并修好。"], ["会员系统改版：积分服务"], [A_CODE, A_TERM, A_CLAUDE]),
        ("改会员等级计算", "你把等级计算从每晚跑批改成积分变动时实时算。", "编码",
         ["等级计算挪进积分变动的事务里，去掉每晚的批处理。", "跑了一遍历史数据对比，和批处理结果一致。"], ["会员系统改版：等级实时计算"], [A_CODE, A_TERM]),
        ("查一个慢查询", "你查了会员列表页 3 秒的慢查询，加了联合索引后降到 80 毫秒。", "数据分析",
         ["看执行计划，按手机号和注册时间筛选时走了全表扫描。", "加联合索引后 80 毫秒，提了变更单。"], ["会员系统改版：会员列表页慢查询"], [A_TERM, A_CHROME, A_CODE]),
        ("数据看板定口径", "你和运营对了会员看板的 6 个指标口径，活跃会员按近 30 天有消费算。", "方案讨论",
         ["活跃会员、复购率、积分核销率等 6 个指标逐个写清定义。", "复购率的分母运营还要再确认。"], ["数据看板：指标口径文档"], [A_LARK, A_CHROME]),
    ]),
    (date(2026, 10, 19), [
        ("会员系统联调", "你和前端联调积分页，修了分页参数和时区两个问题。", "评审验收",
         ["积分明细分页从 1 开始还是从 0 开始两边不一致，统一成从 1 开始。", "过期时间前端按本地时区显示，改成接口直接给带时区的时间。"], ["会员系统改版：联调问题清单"], [A_CHROME, A_CODE, A_LARK]),
        ("看测试报告", "你看了测试提的 14 个问题，当天修掉 9 个。", "评审验收",
         ["14 个问题里 5 个是文案，9 个是逻辑，逻辑的先修。", "剩下 5 个和产品商量后放到上线后。"], ["会员系统改版：测试问题单"], [A_LARK, A_CODE, A_CHROME]),
        ("写数据看板 SQL", "你写完看板的 4 条取数 SQL，活跃会员数和运营手算的对上了。", "数据分析",
         ["4 条 SQL 跑通，活跃会员 12,408，和运营上周手算的差 3 个人，查明是测试账号。", "图表还差复购率一张。"], ["数据看板：取数 SQL"], [A_CHROME, A_TERM, A_CLAUDE]),
    ]),
    (date(2026, 10, 26), [
        ("准备灰度上线", "你写好上线步骤和回滚方案，和运维约了周二晚上灰度 10%。", "写作",
         ["列了 12 步上线步骤，每步写清怎么验证、出问题怎么回滚。", "和运维约好周二 20:00 灰度 10%。"], ["会员系统改版：上线方案"], [A_LARK, A_OBS, A_CHROME]),
        ("盯灰度数据", "你盯着灰度后的接口报错和积分对账，没有发现差异。", "评审验收",
         ["灰度 10% 后接口报错率 0.02%，和旧系统持平。", "积分对账脚本跑了两轮，新旧两边一致。"], ["会员系统改版：灰度监控"], [A_CHROME, A_TERM, A_LARK]),
        ("修灰度发现的问题", "你修了积分过期提醒重复发送的问题，补了一条单测。", "编码",
         ["过期提醒的定时任务在两台机器上都跑了，加了分布式锁。", "补单测，重新发布灰度包。"], ["会员系统改版：过期提醒重复发送"], [A_CODE, A_TERM, A_CLAUDE]),
        ("写上线复盘", "你写了会员系统改版的复盘，列出三条下次要早做的事。", "写作",
         ["复盘：需求阶段积分规则拖了一周，联调前没对齐时区约定，压测做晚了。", "下次需求评审时先把边界例子写全。"], ["会员系统改版：复盘文档"], [A_LARK, A_CLAUDE, A_OBS]),
    ]),
]
WORK_MISC = [
    ("回复飞书消息", "你回了几条飞书消息，确认了下周的排期。", "即时消息", "工作", "", ["回了运营和测试的几条消息，确认下周排期。"], [], [A_LARK, A_WX]),
    ("看简历", "你看了 6 份后端简历，约了两位周四面试。", "阅读", "工作", "招聘", ["6 份简历里两位做过积分或账务系统，约了周四面试。"], ["招聘：后端工程师"], [A_LARK, A_CHROME]),
    ("查资料", "你查了分布式锁的几种做法，记了笔记。", "检索", "工作", "", ["比了数据库行锁、Redis 锁和 ZooKeeper 三种做法。"], [], [A_CHROME, A_OBS]),
]
WEEKLY = ("写周报", "你写完这周的周报：会员系统进度、风险和下周计划。", "写作", "工作", "周报",
          ["按进度、风险、下周三段写，风险写了积分规则未定稿。"], ["周报：本周进度"], [A_LARK, A_OBS])
NOVEL = [
    ("写《雾港》第 {ch} 章", "你写了第 {ch} 章约 {w} 字，主角第一次回到雾港的码头。", "写作", "成长", "写小说《雾港》",
     ["写到主角在码头遇见旧友，对话改了两遍。", "这章约 {w} 字。"], ["写小说《雾港》：第 {ch} 章初稿"], [A_OBS, A_MUSIC]),
    ("改《雾港》第 {ch} 章", "你重读第 {ch} 章，删掉一段多余的回忆，改了结尾。", "写作", "成长", "写小说《雾港》",
     ["删了 800 字回忆，结尾改成主角没开口就离开。"], ["写小说《雾港》：第 {ch} 章修改"], [A_OBS]),
    ("查《雾港》的资料", "你查了上世纪渔港的作息和渔船种类，给第 {ch} 章补细节。", "检索", "成长", "写小说《雾港》",
     ["查了木帆船和机帆船的区别、渔港清晨几点开市。"], ["写小说《雾港》：资料卡"], [A_CHROME, A_OBS]),
]
READ = ("读《置身事内》", "你读了《置身事内》第 {c} 章，记了几条笔记。", "阅读", "成长", "读书笔记",
        ["读第 {c} 章，记下土地财政那一段的三个数字。"], ["读书笔记：《置身事内》"], [A_OBS, A_CHROME])
LEISURE = [
    ("看视频", "你在网页上看了几个视频，没做别的。", "看视频", "休闲", "", ["看了几个讲咖啡烘焙的视频。"], [], [A_CHROME]),
    ("逛论坛", "你逛了技术论坛和资讯站。", "刷信息流", "休闲", "", ["逛了几个帖子，看了两条新闻。"], [], [A_CHROME]),
]
LIFE = [("网上买东西", "你比了几款跑鞋的价格，下了单。", "事务", "生活", "", ["比了三款跑鞋，下单一双。"], [], [A_CHROME]),
        ("订国庆的车票和酒店", "你订了去海川的高铁票和两晚酒店。", "事务", "生活", "国庆出行", ["订了 10-02 去、10-04 回的高铁，订了海边两晚酒店。"], [], [A_CHROME, A_WX])]
CHAT = ("和朋友聊天", "你在微信上和朋友约了周末吃饭。", "即时消息", "关系", "", ["约了周六晚上吃饭。"], [], [A_WX])
GROUP = {"方案讨论": "决策", "评审验收": "决策", "编码": "创作", "写作": "创作", "数据分析": "创作", "阅读": "获取", "检索": "获取",
         "即时消息": "沟通", "刷信息流": "消遣", "看视频": "消遣", "事务": "管理", "配置维护": "管理"}


def phase_tasks(d):
    cur = PHASES[0][1]
    for start, tasks in PHASES:
        if d >= start:
            cur = tasks
    # 最后一周按天排：周一写上线方案，周二晚上灰度，周三盯数据修问题，周四写复盘
    only = {date(2026, 10, 26): ["准备灰度上线"], date(2026, 10, 27): ["准备灰度上线", "盯灰度数据"], date(2026, 10, 28): ["盯灰度数据", "修灰度发现的问题"],
            date(2026, 10, 29): ["写上线复盘", "修灰度发现的问题"]}.get(d)
    return [(t, desc, act, "工作", "数据看板" if "看板" in t else "会员系统改版", rec, ctx, apps) for (t, desc, act, rec, ctx, apps) in cur if not only or t in only]


# ---------- 地点 ----------

def place(label, typ, name, area, district, city="云溪市", guesses=None, lat=30.10, lon=112.20, prefix=None):
    return {"label": label, "type": typ, "name": name, "place_name": name, "admin": {"country": "中国", "province": "", "city": city, "district": district, "area": area},
            "guesses": guesses, "lat": lat, "lon": lon, "city_prefix": prefix}


def g(name, p, typ, dist):
    return {"name": name, "p": p, "type": typ, "distance": dist}


P = {
    "家": place("家", "home", "城东区 · 青石小区", "青石小区", "城东区", lat=30.1012, lon=112.2103),
    "公司": place("公司", "work", "高新区 · 星河科技园", "星河科技园", "高新区", lat=30.1325, lon=112.1688),
    "面馆": place(None, None, "高新区 · 老街面馆", "星河科技园", "高新区", lat=30.1331, lon=112.1702,
                guesses=[g("老街面馆", 0.61, "餐饮服务;中餐厅;面馆", 18), g("便利蜂(星河店)", 0.14, "购物服务;便利店;便利店", 25), g("小杨生煎", 0.09, "餐饮服务;快餐厅;快餐厅", 31)]),
    "健身": place("健身房", None, "城东区 · 动力健身", "青石路", "城东区", lat=30.0987, lon=112.2155),
    "咖啡": place(None, None, "城东区 · 街角咖啡", "青石路", "城东区", lat=30.0995, lon=112.2131,
                guesses=[g("街角咖啡(青石路店)", 0.72, "餐饮服务;咖啡厅;咖啡厅", 12), g("青石书店", 0.11, "购物服务;文化用品店;书店", 40), g("花间花店", 0.05, "购物服务;花鸟鱼虫市场;花店", 35)]),
    "超市": place(None, None, "城东区 · 鲜到家超市", "青石路", "城东区", lat=30.1003, lon=112.2170,
                guesses=[g("鲜到家超市", 0.67, "购物服务;超级市场;超市", 20), g("果园果品", 0.12, "购物服务;综合市场;水果店", 30)]),
    "爸妈家": place("爸妈家", None, "江北区 · 桂花苑", "桂花苑", "江北区", lat=30.1520, lon=112.2401),
    "公园": place(None, None, "江北区 · 滨江公园", "滨江公园", "江北区", lat=30.1488, lon=112.2350,
                guesses=[g("滨江公园", 0.81, "风景名胜;公园广场;公园", 60)]),
    "车站": place(None, None, "城东区 · 云溪东站", "云溪东站", "城东区", lat=30.0920, lon=112.2480,
                guesses=[g("云溪东站", 0.92, "交通设施服务;火车站;火车站", 80)]),
    "酒店": place(None, None, "白沙区 · 海风酒店", "白沙湾", "白沙区", city="海川市", prefix="海川", lat=29.512, lon=113.101,
                guesses=[g("海风酒店(白沙湾店)", 0.83, "住宿服务;宾馆酒店;经济型连锁酒店", 15)]),
    "海边": place(None, None, "白沙区 · 白沙湾海滩", "白沙湾", "白沙区", city="海川市", prefix="海川", lat=29.508, lon=113.110,
                guesses=[g("白沙湾海滩", 0.88, "风景名胜;风景名胜;海滩", 50)]),
    "夜市": place(None, None, "白沙区 · 渔港夜市", "渔港路", "白沙区", city="海川市", prefix="海川", lat=29.515, lon=113.095,
                guesses=[g("渔港夜市", 0.58, "餐饮服务;餐饮相关场所;小吃街", 30), g("阿伟海鲜大排档", 0.21, "餐饮服务;中餐厅;海鲜酒楼", 22)]),
    "海川站": place(None, None, "白沙区 · 海川站", "海川站", "白沙区", city="海川市", prefix="海川", lat=29.530, lon=113.080,
                  guesses=[g("海川站", 0.9, "交通设施服务;火车站;火车站", 90)]),
}


def plan_places(d, rnd):
    """这一天去了哪：[(地点键, 到, 走)]，时刻是这天的小时数（可以超过 24 表示次日）"""
    wd = d.weekday()
    j = lambda x, s=0.15: x + rnd.uniform(-s, s)
    if d in TRIP:
        if d == date(2026, 10, 2):
            return [("家", 0, j(8.6)), ("车站", j(9.1), j(9.6)), ("海川站", j(11.5), j(11.8)), ("酒店", j(12.3), j(14.5)), ("海边", j(14.8), j(18.2)), ("夜市", j(18.6), j(20.6)), ("酒店", j(21.0), 33.0)]
        if d == date(2026, 10, 3):
            return [("酒店", 0, j(9.5)), ("海边", j(10.0), j(16.5)), ("酒店", j(17.0), j(18.5)), ("夜市", j(19.0), j(21.0)), ("酒店", j(21.4), 32.0)]
        return [("酒店", 0, j(11.5)), ("海边", j(12.0), j(14.0)), ("海川站", j(14.6), j(15.1)), ("车站", j(17.0), j(17.3)), ("家", j(18.0), 33.0)]
    if d in HOLIDAY or wd >= 5:
        if wd == 6 or d in (date(2026, 10, 1), date(2026, 10, 6)):
            return [("家", 0, j(10.8)), ("爸妈家", j(11.3), j(15.2)), ("公园", j(15.4), j(17.0)), ("家", j(17.6), 33.5)]
        return [("家", 0, j(9.8)), ("咖啡", j(10.0), j(12.6)), ("超市", j(16.2), j(16.9)), ("家", j(17.0), 32.5)]
    out = [("家", 0, j(8.55, 0.1)), ("公司", j(9.05, 0.08), j(12.05, 0.05))]
    out.append(("面馆", j(12.15, 0.05), j(12.9, 0.05)) if wd in (0, 2, 4) else ("公司", j(12.05, 0.05), j(12.9, 0.05)))
    if wd == 2:
        out += [("公司", j(13.0, 0.05), 13.9), ("咖啡", j(14.15, 0.05), j(17.6)), ("家", j(18.1), 32.6)]
    else:
        out += [("公司", j(13.0, 0.05), j(18.6))]
        out += [("健身", j(19.3), j(20.5)), ("家", j(20.8), 32.6)] if wd in (1, 3) else [("家", j(19.2), 32.6)]
    return out


# ---------- 一天 ----------

def make_day(d, i, rnd, chapter, chap_words):
    wd, today = d.weekday(), d == TODAY
    cut = NOW if today else at(d, 24) + timedelta(hours=6)
    work = wd < 5 and d not in HOLIDAY
    # 电脑：哪几段时间在电脑前、做什么
    blocks = []  # (开始小时, 结束小时, 任务池, 设备)
    tasks = phase_tasks(d)
    if work:
        blocks += [(9.1, 10.0, tasks + WORK_MISC[:1], MAC), (10.0, 12.0, tasks, MAC)]
        if wd == 2:  # 周三下午去咖啡馆干活
            blocks += [(13.05, 13.9, tasks + WORK_MISC[1:], MAC), (14.2, 17.5, tasks, MAC)]
        else:
            blocks += [(13.05, 15.0, tasks + WORK_MISC[1:], MAC), (15.2, 16.2 if wd == 4 else 18.4, tasks, MAC)]
        if wd == 4:
            blocks += [(16.2, 17.2, [WEEKLY], MAC), (17.2, 18.4, tasks, MAC)]
        if wd in (0, 2, 3) and rnd.random() < 0.8:
            blocks += [(21.2, 22.0 + rnd.uniform(0.3, 1.4), NOVEL, WIN)]
        if wd == 4:
            blocks += [(21.5, 22.3, LEISURE + [CHAT], MAC)]
    elif d in TRIP:
        if d == date(2026, 10, 3):
            blocks += [(21.6, 22.2, [("记下海边的见闻", "你在备忘里记了今天在海边看到的几件事，准备写进小说。", "写作", "成长", "写小说《雾港》", ["记了渔船进港、晒网的样子。"], ["写小说《雾港》：素材"], [A_OBS])], MAC)]
    else:
        if wd == 5 or d in (date(2026, 10, 5), date(2026, 10, 7)):
            blocks += [(10.1, 12.5, NOVEL, MAC), (20.5, 21.6, [READ] + LEISURE, WIN)]
        else:
            blocks += [(20.8, 21.8, LEISURE + LIFE[:1] + [CHAT], MAC)]
    if d == date(2026, 9, 24):
        blocks.append((21.0, 21.5, [LIFE[1]], MAC))

    segs = []
    for a, z, pool, dev in blocks:
        t = at(d, 0) + timedelta(minutes=int(a * 6) * 10)
        end = at(d, 0) + timedelta(hours=z)
        task = rnd.choice(pool)
        while t < end and t + timedelta(minutes=10) <= cut:
            if dev == MAC and pool is tasks and rnd.random() < 0.14:  # 开会、走开
                t += timedelta(minutes=10)
                continue
            if rnd.random() < 0.3:
                task = rnd.choice(pool)
            title, desc, act, dom, proj, rec, ctx, apps = task
            fill = {"ch": chapter[0], "w": rnd.choice([1600, 1800, 2100, 2400]), "c": 3 + i // 9}
            if proj == "写小说《雾港》" and title.startswith("写"):
                chap_words[0] += 300
                if chap_words[0] > 4200:
                    chapter[0] += 1
                    chap_words[0] = 0
            f = lambda s: s.format(**fill)
            desc = rnd.choice([desc] + ["你" + r for r in rec if not r.startswith(("这", "用户"))])
            act_sec = rnd.randint(330, 580)
            segs.append({"start": iso(t), "end": iso(t + timedelta(minutes=10)), "kind": "10min", "source": "beayy", "device": dev,
                         "title": f(title), "description": f(desc), "memory": f(desc),
                         "context": [f(c) for c in ctx], "recording": f"### {f(title)}\n\n" + "\n".join(f"- {f(r)}" for r in rec),
                         "apps": apps[:] + ([A_WX] if rnd.random() < 0.3 and A_WX not in apps else []), "active_sec": act_sec,
                         "parts": [{"activity": act, "domain": dom, "project": proj, "share": 1.0}]})
            t += timedelta(minutes=10)
    segs.sort(key=lambda s: s["start"], reverse=True)
    # 每小时：标题取这一小时用得最多的事
    hours = []
    for h in range(24):
        hs = at(d, h)
        if hs >= cut and today:
            break
        inh = [s for s in segs if s["start"] >= iso(hs) and s["start"] < iso(hs + timedelta(hours=1))]
        if not inh:
            if not today or hs + timedelta(hours=1) <= NOW:
                hours.append({"start": iso(hs), "end": iso(hs + timedelta(hours=1)), "title": "不在电脑前", "description": "这一小时没有键鼠操作（离开、锁屏或睡眠）。", "active_sec": 0, "parts": []})
            continue
        if today and hs + timedelta(hours=1) > NOW:
            continue  # 这一小时还没过完：界面用最新一段
        by = {}
        for s in inh:
            by.setdefault(s["title"], []).append(s)
        top = max(by.values(), key=len)[0]
        parts = {}
        for s in inh:
            p = s["parts"][0]
            k = (p["activity"], p["project"])
            parts[k] = parts.get(k, 0) + s["active_sec"]
        hours.append({"start": iso(hs), "end": iso(hs + timedelta(hours=1)), "title": top["title"], "description": top["description"],
                      "active_sec": sum(s["active_sec"] for s in inh), "parts": [{"activity": a, "project": p, "sec": v} for (a, p), v in parts.items()]})
    active = sum(s["active_sec"] for s in segs)
    ah = {}
    for s in segs:
        k = datetime.fromisoformat(s["start"].replace("Z", "+00:00")).astimezone(TZ).strftime("%H")
        ah[k] = ah.get(k, 0) + s["active_sec"]
    first = min((s["start"] for s in segs), default=None)
    last = max((s["end"] for s in segs), default=None)
    fmt = lambda x, m=0: hm_(datetime.fromisoformat(x.replace("Z", "+00:00")) + timedelta(minutes=m)) if x else None
    app_sec = {}
    for s in segs:
        for k, b in enumerate(s["apps"]):
            app_sec[b] = app_sec.get(b, 0) + s["active_sec"] * (0.6 if k == 0 else 0.4 / max(1, len(s["apps"]) - 1))
    late_c = sum(v for k, v in ah.items() if k >= "21")
    computer = {"active_sec": active, "first": fmt(first, rnd.randint(1, 8)), "last": fmt(last, -rnd.randint(0, 7)) if not today else hm_(NOW), "switches": int(active / 3600 * rnd.uniform(75, 110)),
                "active_hours": dict(sorted(ah.items())), "late_sec": late_c,
                "apps": {MAC_APP_NAMES[b]: round(v) for b, v in sorted(app_sec.items(), key=lambda x: -x[1])}}

    # 手机：按时段的权重拿起手机
    weights = {7: 2, 8: 3, 9: 1, 10: 1, 11: 1, 12: 5, 13: 2, 14: 1, 15: 1, 16: 1, 17: 2, 18: 3, 19: 3, 20: 3, 21: 3, 22: 4, 23: 2}
    if not work:
        weights.update({9: 4, 10: 3, 11: 2, 13: 3, 14: 3, 15: 3, 16: 3, 20: 4})
    late_night = rnd.random() < (0.5 if wd in (4, 5) else 0.2)
    target = rnd.uniform(3.4, 4.8) * 3600 if work else rnd.uniform(4.8, 6.6) * 3600
    if d in TRIP:
        target = rnd.uniform(3.0, 4.0) * 3600
    picks = rnd.randint(52, 78) if work else rnd.randint(62, 98)
    if today:
        picks = int(picks * 1.5)
    apps_by_h = lambda h: (["com.tencent.xin", "com.xingin.discover", "com.apple.weather"] if h < 9 else
                           ["com.tencent.weread", "com.netease.cloudmusic", "com.tencent.xin"] if h in (8, 18, 19) and work else
                           ["com.tencent.xin", "com.bytedance.ee.lark"] if work and 9 <= h < 18 and h != 12 else
                           ["com.ss.iphone.ugc.Aweme", "com.xingin.discover", "com.tencent.xin", "com.meituan.itakeaway"] if h in (12, 13) else
                           ["com.ss.iphone.ugc.Aweme", "com.xingin.discover", "com.tencent.weread", "com.zhihu.ios", "com.tencent.xin"] if h >= 21 else
                           ["com.tencent.xin", "com.xingin.discover", "com.taobao.taobao4iphone", "com.ss.iphone.ugc.Aweme", "com.sina.weibo"])
    if d in TRIP:
        apps_by_h = lambda h: ["com.tencent.xin", "com.autonavi.amap", "com.apple.mobileslideshow", "com.xingin.discover", "com.meituan.itakeaway"]
    PN = {"com.tencent.xin": "微信", "com.xingin.discover": "小红书", "com.apple.weather": "天气", "com.tencent.weread": "微信读书", "com.netease.cloudmusic": "网易云音乐",
          "com.bytedance.ee.lark": "飞书", "com.ss.iphone.ugc.Aweme": "抖音", "com.meituan.itakeaway": "美团外卖", "com.zhihu.ios": "知乎",
          "com.taobao.taobao4iphone": "淘宝", "com.sina.weibo": "微博", "com.autonavi.amap": "高德地图", "com.apple.mobileslideshow": "照片"}
    hs_ = list(weights)
    times = sorted(at(d, h, rnd.randint(0, 59), rnd.randint(0, 59)) for h in rnd.choices(hs_, weights=[weights[h] for h in hs_], k=picks))
    if late_night:
        times += sorted(at(d, 24, rnd.randint(0, 50)) + timedelta(minutes=k * 9) for k in range(rnd.randint(3, 7)))
    lens = [rnd.lognormvariate(0, 1.1) for _ in times]
    scale = target / sum(lens)
    sessions, prev_end = [], None
    for t, L in zip(times, lens):
        sec = max(8, int(L * scale))
        if prev_end and t < prev_end:
            t = prev_end + timedelta(seconds=rnd.randint(20, 120))
        if t >= cut or (today and t + timedelta(seconds=sec) > NOW):
            continue
        h = t.astimezone(TZ).hour if t.date() == d else 23
        pool = apps_by_h(h)
        k = 1 if sec < 90 else rnd.randint(1, 3)
        split = sorted(rnd.random() for _ in range(k - 1))
        cuts = [0] + split + [1]
        names = rnd.sample(pool, min(k, len(pool)))
        apps = [{"bundle_id": b, "name": PN[b], "sec": max(1, int(sec * (cuts[j + 1] - cuts[j])))} for j, b in enumerate(names)]
        apps.sort(key=lambda x: -x["sec"])
        sessions.append({"start": iso(t), "end": iso(t + timedelta(seconds=sec)), "sec": sec, "kind": "session", "count": 1, "apps": apps})
        prev_end = t + timedelta(seconds=sec)
    day_end = at(d, 24)
    ph_in = [s for s in sessions if s["start"] < iso(day_end)]
    ph_hours, ph_apps = {}, {}
    for s in ph_in:
        k = datetime.fromisoformat(s["start"].replace("Z", "+00:00")).astimezone(TZ).strftime("%H")
        ph_hours[k] = ph_hours.get(k, 0) + s["sec"]
        for a in s["apps"]:
            ph_apps[a["bundle_id"]] = ph_apps.get(a["bundle_id"], 0) + a["sec"]
    late_p = sum(v for k, v in ph_hours.items() if k >= "21")
    phone = {"total_sec": sum(s["sec"] for s in ph_in), "pickups": len(ph_in), "late_sec": late_p, "night_sec": 0,
             "apps": [{"bundle_id": b, "name": PN[b], "sec": v} for b, v in sorted(ph_apps.items(), key=lambda x: -x[1])],
             "hours": dict(sorted(ph_hours.items())), "synced_until": iso(NOW - timedelta(minutes=47)) if today else iso(day_end + timedelta(hours=2)),
             "sessions": sorted(ph_in, key=lambda s: s["start"], reverse=True)}
    after = [s for s in sessions if s["start"] >= iso(day_end)]  # 过了零点的那几次算第二天凌晨

    # 地点
    visits, plan = [], []
    for k, a, z in plan_places(d, rnd):
        if plan and plan[-1][0] == k:
            plan[-1] = (k, plan[-1][1], z)
        else:
            plan.append((k, a, z))
    for n, (k, a, z) in enumerate(plan):
        p = P[k]
        st = at(d, 0) + timedelta(hours=a)
        en = at(d, 0) + timedelta(hours=z)
        if st >= cut:
            break
        v = {"id": f"visit-{d:%Y%m%d}-{n}", "device": PHONE, "source": "visit", "acc": round(rnd.uniform(5, 25), 1),
             "start": iso(st), "end": None if (today and en > NOW) else iso(en), "arrive": iso(st), "arrive_known": a > 0,
             "label": p["label"], "type": p["type"], "name": p["name"], "place_name": p["place_name"], "admin": p["admin"],
             "city_prefix": p["city_prefix"], "lat": p["lat"] + rnd.uniform(-2e-4, 2e-4), "lon": p["lon"] + rnd.uniform(-2e-4, 2e-4),
             "guesses": p["guesses"], "confirmed": "auto" if p["guesses"] and p["guesses"][0]["p"] >= 0.6 else None,
             "doing": {"kind": p["guesses"][0]["type"].split(";")[0], "p": p["guesses"][0]["p"]} if p["guesses"] else None, "shops": []}
        if a == 0:
            v["arrive"] = iso(at(d, 0) - timedelta(hours=rnd.uniform(3, 6)))
        visits.append(v)

    out = {"date": d.isoformat(), "computers": sorted({s["device"] for s in segs}) or [MAC], "device_names": {MAC: "MacBook Pro", WIN: "书房台式机"},
           "segments": segs, "hours": hours, "phone": phone, "places": {"visits": visits}, "speech": None, "summary_md": None,
           "work": work_tree(segs)}
    if today:
        spans = [[s["start"], (datetime.fromisoformat(s["start"].replace("Z", "+00:00")) + timedelta(seconds=s["active_sec"])).strftime("%Y-%m-%dT%H:%M:%SZ")] for s in segs]
        out["live"] = {**{k: computer[k] for k in ("active_sec", "first", "last", "switches", "active_hours")}, "spans": spans,
                       "apps": [{"name": MAC_APP_NAMES[b], "bundle_id": b, "sec": round(v)} for b, v in sorted(app_sec.items(), key=lambda x: -x[1])]}
        out["pack"] = None
    else:
        out["live"] = None
        out["pack"] = {"date": d.isoformat(), "partial": False, "generated_at": f"{d + timedelta(days=1)} 00:10", "computer": computer,
                       "phone": {k: phone[k] for k in ("total_sec", "pickups", "late_sec", "hours")}}
    return out, after


def work_tree(segs):
    def tree(key):
        agg = {}
        for s in segs:
            for p in s["parts"]:
                top = GROUP[p["activity"]] if key == "activities" else p["domain"]
                child = p["activity"] if key == "activities" else (p["project"] or "未归项目")
                agg.setdefault(top, {}).setdefault(child, 0)
                agg[top][child] += s["active_sec"] * p["share"]
        return sorted([{"name": t, "sec": round(sum(v.values())), "children": sorted([{"name": c, "sec": round(x)} for c, x in v.items()], key=lambda r: -r["sec"])}
                       for t, v in agg.items()], key=lambda r: -r["sec"])
    tagged = sum(s["active_sec"] for s in segs)
    return {"domains": tree("domains"), "activities": tree("activities"), "tagged_sec": tagged, "untagged_sec": 0}


# ---------- 总结（照 Beayy 日总结的格式） ----------

LINE = {  # 一句话里白天那半句（按这天最后在做的事）
    "会员系统需求评审": "会员系统的需求过了一遍，积分规则还差例子", "梳理旧会员接口": "旧会员接口理清了，9 个能直接复用",
    "和 Claude 讨论表结构": "积分流水表定了按月分表", "写积分服务": "积分服务写到发放和扣减", "改会员等级计算": "等级计算改成了实时算",
    "查一个慢查询": "会员列表页的慢查询降到 80 毫秒", "数据看板定口径": "数据看板的指标口径对齐了", "会员系统联调": "积分页联调修掉两个问题",
    "看测试报告": "测试提的问题修掉大半", "写数据看板 SQL": "看板的取数 SQL 跑通了", "准备灰度上线": "上线方案写好，约了灰度",
    "盯灰度数据": "灰度 10% 上去了，报错率和旧系统持平", "修灰度发现的问题": "修掉过期提醒重复发送", "写上线复盘": "会员系统的复盘写完了",
    "回复飞书消息": "白天多在回消息", "看简历": "看了一下午简历", "查资料": "查了一天资料", "写周报": "周报交了",
}

NEXT = {  # 明天那一条（按这天最后在做的事）
    "会员系统需求评审": "等产品补上积分过期的例子", "梳理旧会员接口": "把要拆的 6 个接口排个先后", "和 Claude 讨论表结构": "按月分表写成设计文档发评审",
    "写积分服务": "补积分过期的单测", "改会员等级计算": "跑一遍全量历史数据对比", "查一个慢查询": "盯一下索引上线后的耗时", "数据看板定口径": "问运营复购率的分母",
    "会员系统联调": "和前端对完剩下的接口", "看测试报告": "剩下 5 个问题和产品定上线前修哪些", "写数据看板 SQL": "补复购率那张图",
    "准备灰度上线": "晚上 20:00 灰度 10%", "盯灰度数据": "再跑一轮积分对账", "修灰度发现的问题": "重新发灰度包，盯一晚", "写上线复盘": "复盘发给组里，约全量上线时间",
}

QUOTES = [  # 确实存在的原句和出处
    ("不积跬步，无以至千里", "《荀子·劝学》"), ("凡是过去，皆为序章", "莎士比亚《暴风雨》"), ("行百里者半九十", "《战国策·秦策》"),
    ("业精于勤，荒于嬉", "韩愈《进学解》"), ("纸上得来终觉浅，绝知此事要躬行", "陆游《冬夜读书示子聿》"), ("偷得浮生半日闲", "李涉《题鹤林寺僧舍》"),
    ("工欲善其事，必先利其器", "《论语·卫灵公》"), ("我们必须想象西西弗斯是幸福的", "加缪《西西弗神话》"), ("人生如逆旅，我亦是行人", "苏轼《临江仙·送钱穆父》"),
    ("天下难事，必作于易", "《老子》"), ("行到水穷处，坐看云起时", "王维《终南别业》"), ("明日复明日，明日何其多", "钱福《明日歌》"),
    ("知之者不如好之者，好之者不如乐之者", "《论语·雍也》"), ("宝剑锋从磨砺出，梅花香自苦寒来", "《警世贤文》"), ("采菊东篱下，悠然见南山", "陶渊明《饮酒·其五》"),
    ("温故而知新，可以为师矣", "《论语·为政》"), ("人生到处知何似，应似飞鸿踏雪泥", "苏轼《和子由渑池怀旧》"), ("面朝大海，春暖花开", "海子《面朝大海，春暖花开》"),
    ("每一个不曾起舞的日子，都是对生命的辜负", "尼采"), ("生活不是我们活过的日子，而是我们记住的日子", "马尔克斯《活着为了讲述》"),
    ("慎终如始，则无败事", "《老子》"), ("吾生也有涯，而知也无涯", "《庄子·养生主》"), ("海内存知己，天涯若比邻", "王勃《送杜少府之任蜀州》"),
    ("路漫漫其修远兮，吾将上下而求索", "屈原《离骚》"),
]
SEA_QUOTE = 17


USED = []  # 最近几天一句话用过的白天那半句


def day_story(d, x, qi):
    """日总结 md：一句话、点评、时间、做成了什么、偏离、明天"""
    segs, ph, visits = x["segments"], x["phone"], x["places"]["visits"]
    c = x["pack"]["computer"]
    work = d.weekday() < 5 and d not in HOLIDAY
    titles = []
    for s in sorted(segs, key=lambda s: s["start"]):
        if s["title"] not in titles:
            titles.append(s["title"])
    by_title = {}
    for s in segs:
        by_title.setdefault(s["title"], s)
    novel = [t for t in titles if "雾港" in t]
    main = [t for t in titles if by_title[t]["parts"][0]["project"] in ("会员系统改版", "数据看板")]
    top_app = ph["apps"][0]["name"] if ph["apps"] else "微信"
    places = []
    for v in visits:
        n = v["label"] or v["guesses"][0]["name"].split("(")[0]
        if n not in places:
            places.append(n)
    gym = "健身房" in places
    if d in TRIP:
        line = {2: "坐高铁到了海川，下午在海边待到傍晚", 3: "一整天在白沙湾，电脑几乎没开", 4: "上午又去了趟海边，傍晚回到家"}[d.day]
    elif work:
        fresh = [t for t in reversed(main) if LINE.get(t) not in USED[-2:]] or [t for t, *_ in phase_tasks(d) if LINE.get(t) not in USED[-2:]]
        lead = LINE.get((fresh or main or [""])[0], "白天多是杂事")
        USED.append(lead)
        evening = [s for s in segs if s["start"] >= iso(at(d, 21)) and "雾港" not in s["title"]]
        tail = "晚上写了一段《雾港》" if novel else "晚上在电脑上看了会儿视频" if evening else "晚上去了健身房" if gym else f"睡前刷{top_app}到很晚" if ph["late_sec"] > 3000 else "晚上没再开电脑"
        line = f"{lead}，{tail}"
    else:
        line = ("上午在咖啡馆写《雾港》，下午买菜，晚上歇着" if "咖啡馆" in "".join(places) or any("街角" in p for p in places)
                else f"中午去爸妈家吃饭，下午在滨江公园走了走，手机用了 {ph['total_sec'] / 3600:.1f} 小时")
    q, src = QUOTES[SEA_QUOTE] if d == date(2026, 10, 3) else QUOTES[qi % len(QUOTES)]
    if d in TRIP:
        remark = "整天在海边，看渔船进港、晒网，晚上在渔港夜市吃了海鲜。电脑只开了一会儿记素材，手机多是地图和照片；难得一天，时间不归哪个项目管。"
    elif work:
        remark = (f"白天{'、'.join(main[:2]) or '杂事'}一件接一件，{'晚上又回到书桌写小说，' if novel else '晚上去健身房出了身汗，' if gym else ''}"
                  f"在电脑前 {c['active_sec'] / 3600:.1f} 小时，切换 app {c['switches']} 次。{top_app}占了手机的大头，{'睡前那一段尤其长' if ph['late_sec'] > 2400 else '好在睡前收得住'}。")
    else:
        remark = (f"周末的节奏慢下来：{'、'.join(places[:3])}几处走走停停，电脑只开了 {c['active_sec'] / 3600:.1f} 小时。"
                  f"手机 {ph['total_sec'] / 3600:.1f} 小时，{top_app}最多，{'夜里还刷到了零点以后' if ph['late_sec'] > 3000 else '晚上没熬太晚'}。")
    done = []
    for t in titles[:6]:
        s = by_title[t]
        if s["parts"][0]["domain"] in ("休闲",) or s["parts"][0]["activity"] == "即时消息":
            continue
        done.append(s["description"].removeprefix("你").rstrip("。") + "。")
    off = []
    if ph["total_sec"] > 4.5 * 3600:
        off.append(f"手机 {ph['total_sec'] / 3600:.1f} 小时，{top_app} {dur(ph['apps'][0]['sec'])}。")
    if ph["late_sec"] > 2400:
        off.append(f"21:00 以后手机用了 {dur(ph['late_sec'])}，睡得比打算的晚。")
    if c["switches"] > 600:
        off.append(f"电脑切换 app {c['switches']} 次，写东西时常被消息打断。")
    if not off:
        off.append("没有明显偏离。")
    nxt = {True: [NEXT.get(main[-1] if main else "", "接着推进会员系统改版"), "《雾港》再写一千字" if not novel else "把今天写的那章再读一遍"],
           False: ["早点睡，明天上班", "周一先看测试和运营的消息"]}[work]
    if d.weekday() == 4:
        nxt = ["周末去咖啡馆写《雾港》", "下周一先过一遍测试问题单"]
    plc = "、".join(f"{p}" for p in places[:5])
    md = f"""# {d:%m-%d} 周{WD[d.weekday()]}

> 由 Beayy 用当天的记录生成，数据截至 {d + timedelta(days=1):%m-%d} 00:15。

## 今天的一句话

{line}

## 点评

「{q}」（{src}）{remark}

## 时间都去哪了

- 在电脑前 {c['active_sec'] / 3600:.1f} 小时（{c['first'] or '—'}–{c['last'] or '—'}），切换 app {c['switches']} 次
- 手机 {ph['total_sec'] / 3600:.1f} 小时，拿起 {ph['pickups']} 次
- 去了 {len(places)} 个地方：{plc}

## 做成了什么

{chr(10).join('- ' + x for x in done) or '- 没有特别做成的事。'}

## 偏离

{chr(10).join('- ' + x for x in off)}

## 明天

{chr(10).join('- ' + x for x in nxt)}
"""
    return md, line


WEEK_HEAD = {  # 这一周白天的主线（周一 → 半句话；月总结「做成了什么」也用它）
    date(2026, 9, 21): "会员系统的需求和表结构定下来了", date(2026, 10, 5): "节后开写积分服务，等级改成实时算",
    date(2026, 10, 12): "积分服务写完，看板口径对齐了", date(2026, 10, 19): "会员系统联调、修测试问题，看板 SQL 跑通",
}


def week_story(mon, days, lines, qi):
    xs = [days[k] for k in sorted(days) if mon <= date.fromisoformat(k) <= mon + timedelta(days=6)]
    c = sum((x["live"] or x["pack"]["computer"])["active_sec"] for x in xs) / 3600
    p = sum(x["phone"]["total_sec"] for x in xs) / 3600
    late = sum(1 for x in xs if x["phone"]["late_sec"] > 3600)
    trip = any(date.fromisoformat(x["date"]) in TRIP for x in xs)
    phase = phase_tasks(mon + timedelta(days=3))[0][0]
    novel_days = sum(1 for x in xs if any("雾港" in s["title"] for s in x["segments"]))
    if trip:
        line, body = "国庆放了七天：去海边玩了三天，其余在家写小说、陪爸妈", "假期七天，电脑大多时候合着，海边三天手机全是地图和照片。回来后两个上午在咖啡馆写《雾港》，进度没停。"
    else:
        head = WEEK_HEAD.get(mon, f"{phase}占了大半周")
        line = f"{head}，《雾港》写了 {novel_days} 个晚上"
        body = f"这周白天围着会员系统转，{phase}是主线；晚上有 {novel_days} 天回到书桌写《雾港》。手机一周 {p:.0f} 小时，有 {late} 晚睡前刷到很晚。"
    q, src = QUOTES[qi % len(QUOTES)]
    return f"""# {mon:%m-%d} – {mon + timedelta(days=6):%m-%d} 周总结

> 由 Beayy 用这一周的 {len(xs)} 份日总结生成，数据截至 {mon + timedelta(days=7):%m-%d} 00:20。

## 这周的一句话

{line}

## 点评

「{q}」（{src}）{body}

## 做成了什么

{chr(10).join('- ' + l + '。' for l in lines[:5])}

## 时间和作息

- 在电脑前共 {c:.1f} 小时，手机共 {p:.1f} 小时
- {late} 晚 21:00 以后手机超过 1 小时

## 偏离

- 睡前刷手机的晚上有 {late} 个，比上周{'多' if late > 2 else '少'}。

## 下周

- {'会员系统接着往上线推' if not trip else '收心，回到会员系统的编码'}
- 《雾港》保持每周三个晚上
""", line


def month_story(first, days, wlines, qi):
    xs = [days[k] for k in sorted(days) if k[:7] == first.isoformat()[:7]]
    c = sum((x["live"] or x["pack"]["computer"])["active_sec"] for x in xs) / 3600
    p = sum(x["phone"]["total_sec"] for x in xs) / 3600
    q, src = QUOTES[qi % len(QUOTES)]
    oct_ = first.month == 10
    line = f"会员系统从写代码推到灰度上线，国庆去了趟海边，《雾港》写到第 {CH[0]} 章" if oct_ else "会员系统改版开了头，需求和表结构定下来，《雾港》断断续续地写"
    body = ("月初的假期把节奏放慢，海边三天几乎没碰电脑；回来后积分服务、联调、灰度一路推下去，月底总算上线。晚上的书桌留给了《雾港》，"
            "章节一章章往前挪。手机仍是最大的漏洞，睡前那一段常常收不住。") if oct_ else "这个月只记了最后十来天：需求评审、旧接口清单、表结构，都是动手前的功课。晚上偶尔写小说，节奏还没起来。"
    return f"""# {first:%Y-%m} 月总结

> 由 Beayy 用这个月的 {len(xs)} 份日总结生成，数据截至 {min(TODAY, date(first.year, first.month, 28) + timedelta(days=4)):%m-%d} 16:00。

## 这个月的一句话

{line}

## 点评

「{q}」（{src}）{body}

## 做成了什么

{chr(10).join('- ' + l + '。' for l in wlines)}
- 《雾港》写到第 {CH[0]} 章。

## 时间和作息

- 在电脑前共 {c:.0f} 小时，手机共 {p:.0f} 小时，记了 {len(xs)} 天
- 工作日平均 9:05 开始用电脑，18:30 前后合上

## 偏离

- 睡前刷手机仍是最大的偏离，周五、周六尤其多。

## 下个月

- {'会员系统全量上线，跟进看板' if oct_ else '积分服务写完，开始联调'}
- 《雾港》写完第一部分
""", line


# ---------- 生成 ----------

rnd = random.Random(29)
chapter, words = [7], [0]
CH = chapter
days, carry = {}, []
for i in range((TODAY - FIRST).days + 1):
    d = FIRST + timedelta(days=i)
    x, after = make_day(d, i, rnd, chapter, words)
    # 前一天过了零点刷的手机，算这天凌晨（night_sec）
    x["phone"]["night_sec"] = sum(s["sec"] for s in carry)
    x["phone"]["sessions"] = sorted(carry, key=lambda s: s["start"], reverse=True) + x["phone"]["sessions"] if carry else x["phone"]["sessions"]
    x["phone"]["sessions"].sort(key=lambda s: s["start"], reverse=True)
    x["phone"]["total_sec"] += x["phone"]["night_sec"]
    x["phone"]["pickups"] += len(carry)
    for s in carry:
        k = datetime.fromisoformat(s["start"].replace("Z", "+00:00")).astimezone(TZ).strftime("%H")
        x["phone"]["hours"][k] = x["phone"]["hours"].get(k, 0) + s["sec"]
    x["phone"]["hours"] = dict(sorted(x["phone"]["hours"].items()))
    if x["pack"]:
        x["pack"]["phone"].update(total_sec=x["phone"]["total_sec"], pickups=x["phone"]["pickups"], hours=x["phone"]["hours"])
    carry = after
    days[d.isoformat()] = x

lines, qi = {}, 0
for k, x in days.items():
    d = date.fromisoformat(k)
    if d == TODAY:
        continue
    x["summary_md"], lines[k] = day_story(d, x, qi)
    qi += 1

weeks, wlines = {}, {}
mon = FIRST - timedelta(days=FIRST.weekday())
while mon <= TODAY:
    sun = mon + timedelta(days=6)
    dd = [{"date": (mon + timedelta(days=k)).isoformat(), "line": lines.get((mon + timedelta(days=k)).isoformat())} for k in range(7)]
    md, wl = (None, None)
    if sun < TODAY and mon >= FIRST - timedelta(days=6):
        done = []
        for k in range(7):
            x = days.get((mon + timedelta(days=k)).isoformat())
            for s in (x or {}).get("segments", []):
                t = s["description"].removeprefix("你").rstrip("。")
                if s["parts"][0]["domain"] in ("工作", "成长") and t not in done:
                    done.append(t)
        rnd.shuffle(done)
        md, wl = week_story(mon, days, done, qi)
        qi += 3
    weeks[mon.isoformat()] = {"monday": mon.isoformat(), "sunday": sun.isoformat(), "summary_md": md, "error": None, "days": dd}
    wlines[mon.isoformat()] = wl
    mon += timedelta(days=7)

months = {}
for first in sorted({date.fromisoformat(k).replace(day=1) for k in days}):
    last = (first.replace(day=28) + timedelta(days=4)).replace(day=1) - timedelta(days=1)
    ws = []
    m = first - timedelta(days=first.weekday())
    while m <= last:
        ws.append({"monday": m.isoformat(), "sunday": (m + timedelta(days=6)).isoformat(), "line": wlines.get(m.isoformat())})
        m += timedelta(days=7)
    heads = [WEEK_HEAD.get(date.fromisoformat(x["monday"]), "国庆去海边玩了三天" if date.fromisoformat(x["monday"]) == date(2026, 9, 28) else None) for x in ws if wlines.get(x["monday"])]
    md, _ = month_story(first, days, [h for h in heads if h], qi)
    qi += 5
    months[first.isoformat()[:7]] = {"first": first.isoformat(), "last": last.isoformat(), "summary_md": md, "error": None, "weeks": ws}

usage = []
for k in sorted(days)[-30:]:
    x = days[k]
    segs = len(x["segments"])
    kinds = {"segment": {"in": segs * 4300, "out": segs * 720, "calls": segs},
             "hourly": {"in": len([h for h in x["hours"] if h["active_sec"]]) * 2600, "out": len([h for h in x["hours"] if h["active_sec"]]) * 420, "calls": len([h for h in x["hours"] if h["active_sec"]])}}
    if x["summary_md"]:
        kinds["daily"] = {"in": 21000, "out": 1500, "calls": 1}
    if date.fromisoformat(k).weekday() == 0:
        kinds["weekly"] = {"in": 9000, "out": 1300, "calls": 1}
    if k.endswith("-01"):
        kinds["monthly"] = {"in": 12000, "out": 1600, "calls": 1}
    usage.append({"date": k, "in": sum(v["in"] for v in kinds.values()), "out": sum(v["out"] for v in kinds.values()), "kinds": kinds})

proj = {}
for x in days.values():
    for s in x["segments"]:
        p = s["parts"][0]
        if p["project"]:
            r = proj.setdefault(p["project"], {"name": p["project"], "domain": p["domain"], "sec": 0, "last": x["date"], "aliases": []})
            r["sec"] += s["active_sec"]
            r["last"] = max(r["last"], x["date"])
proj["会员系统改版"]["aliases"] = ["会员改版", "积分系统"]
proj["写小说《雾港》"]["aliases"] = ["雾港"]


def named(key, idx, name):
    vs = [v for x in days.values() for v in x["places"]["visits"] if (v["label"] or "") == name]
    sec = sum((datetime.fromisoformat((v["end"] or iso(NOW)).replace("Z", "+00:00")) - datetime.fromisoformat(v["start"].replace("Z", "+00:00"))).total_seconds() for v in vs[-60:])
    p = P[key]
    return {"index": idx, "name": name, "type": p["type"], "city": p["admin"]["city"], "district": p["admin"]["district"], "address": None,
            "count": len({v["start"][:10] for v in vs}), "sec": sec, "last": max(v["start"][:10] for v in vs), "lat": p["lat"], "lon": p["lon"], "r": 150.0}


mac_first = iso(at(FIRST, 9, 6))
mock = {
    "list_days": [{"date": k, "line": lines.get(k), "active_sec": (x["live"] or x["pack"]["computer"])["active_sec"], "phone_sec": x["phone"]["total_sec"],
                   "hc": (x["live"] or x["pack"]["computer"])["active_hours"], "hp": x["phone"]["hours"]} for k, x in sorted(days.items(), reverse=True)],
    "get_day": days,
    "get_week": weeks,
    "get_month": months,
    "llm_usage": {"official": False, "days": usage},
    "get_status": {"permissions": {"accessibility": True, "input_monitoring": True, "full_disk_access": True}, "capturing": True, "paused": False, "paused_until": None,
                   "events_today": 18342, "phone_sync": f"{PHONE} 新增 12 条", "relay_sync": "新增 0 条", "device": MAC,
                   "version": "0.2.27", "shadow": False, "login_item": True, "exe": "/Applications/Beayy.app/Contents/MacOS/beayy"},
    "get_settings": {"gateway": "https://api.deepseek.com/v1", "model_segment": "deepseek-chat", "model_daily": "deepseek-chat", "repo_path": "",
                     "blocklist": ["1Password", "钥匙串访问"], "has_api_key": True, "api_key_source": "keychain", "onboarded": True,
                     "relay_server": "https://audio.beayy.com", "has_relay_token": True, "default_gateway": "https://api.deepseek.com/v1", "default_model": "deepseek-chat",
                     "sync_server": "https://sync.beayy.com", "default_sync_server": "https://sync.beayy.com", "sync_mode": "server", "sync_folder": "",
                     "icloud_folder": "/Users/demo/Library/Mobile Documents/com~apple~CloudDocs/Beayy",
                     "transcribe_server": "https://audio.beayy.com", "default_transcribe_server": "https://audio.beayy.com", "speaker_name": "我",
                     "model_mode": "custom", "map_mode": "custom", "has_amap_key": False, "has_amap_js": False},
    "llm_status": {"plan": None, "month": TODAY.isoformat()[:7], "llm": {"used": sum(u["in"] + u["out"] for u in usage if u["date"][:7] == TODAY.isoformat()[:7]), "quota": 20000000},
                   "asr": {"used": 0, "quota": 1800000}, "map": {"used": 86, "quota": 30000}},
    "get_rules": [{"id": "r1", "text": "23:00 以后不用手机", "enabled": True}, {"id": "r2", "text": "手机每天不超过 4 小时", "enabled": True},
                  {"id": "r3", "text": "抖音每天不超过 30 分钟", "enabled": True}, {"id": "r4", "text": "工作日 22:30 以后不开电脑", "enabled": False}],
    "list_devices": [{"id": MAC, "kind": "mac", "name": "MacBook Pro", "this": True, "synced": False, "first": mac_first, "last": iso(NOW), "events": 612408, "bytes": 168_400_000},
                     {"id": WIN, "kind": "win", "name": "书房台式机", "custom": True, "this": False, "synced": True, "first": iso(at(FIRST, 21, 12)), "last": iso(at(TODAY - timedelta(days=1), 22, 41)), "events": 88213, "bytes": 21_700_000},
                     {"id": PHONE, "kind": "iphone", "name": "iPhone", "this": False, "synced": False, "first": iso(at(FIRST, 0, 2)), "last": iso(NOW - timedelta(minutes=47)), "events": 41320, "bytes": 6_100_000}],
    "sync_status": {"configured": True, "email": "demo@example.com", "server": "https://sync.beayy.com", "user": "demo", "last": iso(NOW - timedelta(minutes=3)),
                    "last_error": None, "uploaded": 3480, "need_login": False, "running": False, "shadow": False},
    "account_info": {"session": "s1", "sessions": [{"id": "s1", "device": MAC, "name": "MacBook Pro", "last_seen": int((NOW - timedelta(minutes=3)).timestamp() * 1000)},
                                                   {"id": "s2", "device": WIN, "name": "书房台式机", "last_seen": int(at(TODAY - timedelta(days=1), 22, 41).timestamp() * 1000)}]},
    "signup_info": {"open": True, "invite": False, "quota": 100_000_000},
    "folder_state": {"vault": False},
    "get_profile": {"name": "小舟", "avatar": None, "email": "demo@example.com"},
    "list_projects": sorted(proj.values(), key=lambda r: -r["sec"]),
    "place_stats": {"days": 30, "named": [named("家", 0, "家"), named("公司", 1, "公司"), named("爸妈家", 2, "爸妈家"), named("健身", 3, "健身房")],
                    "frequent": [{"name": "街角咖啡", "city": "云溪市", "district": "城东区", "count": 9, "sec": 9 * 3 * 3600, "last": (TODAY - timedelta(days=1)).isoformat()},
                                 {"name": "老街面馆", "city": "云溪市", "district": "高新区", "count": 12, "sec": 12 * 2700, "last": (TODAY - timedelta(days=3)).isoformat()}]},
    "list_place_labels": [],
    "storage_stats": {"local": {"path": "/Users/demo/Library/Application Support/Beayy", "total": 214_600_000, "items": [
        {"key": "computer", "name": "电脑记录", "bytes": 190_100_000, "keep": "长期"}, {"key": "summaries", "name": "10 分钟摘要", "bytes": 3_400_000, "keep": "长期"},
        {"key": "phone", "name": "手机使用和停留点", "bytes": 6_100_000, "keep": "长期"}, {"key": "daily", "name": "日、周、月总结", "bytes": 420_000, "keep": "长期"},
        {"key": "places", "name": "地点缓存", "bytes": 60_000, "keep": "长期"}, {"key": "other", "name": "其他", "bytes": 14_500_000, "keep": "长期"}]},
        "cloud": {"server": "https://sync.beayy.com", "total": 31_200_000, "items": [
            {"key": "segment", "name": "电脑和手机记录", "count": 3480, "bytes": 29_800_000, "oldest": None, "keep": "长期"},
            {"key": "summary", "name": "10 分钟摘要", "count": 1630, "bytes": 1_200_000, "oldest": None, "keep": "长期"},
            {"key": "daily", "name": "日、周、月总结", "count": 46, "bytes": 190_000, "oldest": None, "keep": "长期"},
            {"key": "account", "name": "账号设置", "count": 2, "bytes": 2000, "oldest": None, "keep": "长期"}]},
        "transcribe": {"server": "https://audio.beayy.com", "jobs": {}, "bytes": 0, "keep": "音频转完就删，结果取走就删，没取走的最多 7 天"},
        "repo": None, "at": int(NOW.timestamp() * 1000)},
}
# 设备详情 · 事件：本机最近几分钟
ev, t = [], NOW
for k, (typ, extra) in enumerate([("keyboard.text_input", {"app_name": "飞书", "text": "复盘先列三条：需求、时区约定、压测"}), ("app.activated", {"app_name": "飞书", "window_title": "会员系统改版复盘"}),
                                   ("mouse.click", {"app_name": "Google Chrome", "element": {"role": "AXLink", "title": "灰度监控面板"}}), ("ui.text", {"app_name": "Google Chrome", "lines": 46}),
                                   ("app.activated", {"app_name": "Google Chrome", "window_title": "灰度监控 · 会员服务"}), ("mouse.scroll", {"app_name": "Claude", "ticks": 6}),
                                   ("keyboard.submit", {"app_name": "Claude", "text": "帮我把这三条复盘写成一段话，不要套话", "keys": {"human": 22}}), ("ui.text", {"app_name": "Claude", "lines": 31}),
                                   ("app.activated", {"app_name": "Claude", "window_title": "Claude"}), ("summary.run", {"written": 1, "seconds": 9}),
                                   ("sync.run", {"uploaded": 2, "downloaded": 0}), ("presence", {"state": "active"}), ("screen.unlocked", {}),
                                   ("presence", {"state": "idle", "idle_seconds": 840}), ("keyboard.shortcut", {"app_name": "Visual Studio Code", "keys": "⌘S"}),
                                   ("keyboard.text_input", {"app_name": "Visual Studio Code", "text": "if acquired { run_expire_notice() }"}),
                                   ("window.changed", {"app_name": "Visual Studio Code", "window_title": "notice.rs — member-service"}), ("phone.sync", {"added": 12})]):
    t -= timedelta(seconds=rnd.randint(8, 70) if typ != "presence" or extra.get("state") != "idle" else 840)
    ev.append({"timestamp": iso(t), "event_type": typ, "device": MAC, **extra})
mock["recent_events"] = {"events": ev, "more": True}

# 收支：两个月的账单（微信、支付宝各一份，金额商户都是编的），格式同 get_bills，按月份存
brnd = random.Random(7)
SHOPS = [("街角咖啡", "餐饮美食", "拿铁", 2800, 3800), ("老街面馆", "餐饮美食", "牛肉面", 1800, 2600), ("云溪地铁", "交通出行", "地铁出行", 300, 600),
         ("便利蜂", "日用百货", "饮料", 500, 1800), ("铁馆健身", "运动户外", "私教课", 28000, 28000), ("小区菜场", "餐饮美食", "买菜", 3000, 9000)]
bills = []
for k in range(70):
    d = TODAY - timedelta(days=brnd.randint(0, 55))
    who, cat, item, lo, hi = brnd.choice(SHOPS)
    src = brnd.choice(["wechat", "alipay"])
    bills.append({"src": src, "time": f"{d} {brnd.randint(8, 22):02d}:{brnd.randint(0, 59):02d}:00", "dir": "out", "fen": brnd.randint(lo, hi) // 10 * 10,
                  "category": cat if src == "alipay" else "商户消费", "counterparty": who, "item": item, "method": "零钱" if src == "wechat" else "余额宝", "status": "支付成功" if src == "wechat" else "交易成功"})
for m in {(TODAY - timedelta(days=n)).isoformat()[:7] for n in range(0, 56)}:
    bills.append({"src": "alipay", "time": f"{m}-10 09:00:00", "dir": "in", "fen": 1250000, "category": "其他", "counterparty": "云溪科技", "item": "工资", "method": "", "status": "交易成功"})
    bills.append({"src": "alipay", "time": f"{m}-05 10:00:00", "dir": "out", "fen": 320000, "category": "住房物业", "counterparty": "安居房屋", "item": "房租", "method": "余额宝", "status": "交易成功"})
    bills.append({"src": "alipay", "time": f"{m}-12 21:00:00", "dir": "out", "fen": 59900, "category": "日用百货", "counterparty": "某网店", "item": "台灯", "method": "余额宝", "status": "交易关闭"})
    bills.append({"src": "alipay", "time": f"{m}-15 03:00:00", "dir": "none", "fen": 200000, "category": "投资理财", "counterparty": "余额宝", "item": "单次转入", "method": "银行卡", "status": "交易成功"})
bills.sort(key=lambda b: b["time"], reverse=True)
by_month = {}
for i, b in enumerate(bills):
    m = b["time"][:7]
    x = by_month.setdefault(m, {"month": m, "spend_fen": 0, "income_fen": 0, "refund_fen": 0, "count": 0, "days": {}, "to": {}, "items": []})
    refund = b["fen"] if b["status"] == "交易关闭" else 0
    counts = b["dir"] != "none"
    out, inc = (b["fen"] - refund if b["dir"] == "out" else 0), (b["fen"] if b["dir"] == "in" else 0)
    if counts:
        x["count"] += 1; x["spend_fen"] += out; x["income_fen"] += inc; x["refund_fen"] += refund
        dd = x["days"].setdefault(b["time"][:10], [0, 0]); dd[0] += out; dd[1] += inc
        if out: t = x["to"].setdefault(b["counterparty"], [0, 0]); t[0] += out; t[1] += 1
    yuan_ = f"{b['fen'] / 100:.2f}"
    raw = ({"交易时间": b["time"], "交易类型": b["category"], "交易对方": b["counterparty"], "商品": b["item"], "收/支": {"out": "支出", "in": "收入"}.get(b["dir"], "/"),
            "金额(元)": "¥" + yuan_, "支付方式": b["method"], "当前状态": b["status"], "交易单号": f"4200000{i:023d}"} if b["src"] == "wechat" else
           {"交易时间": b["time"], "交易分类": b["category"], "交易对方": b["counterparty"], "商品说明": b["item"], "收/支": {"out": "支出", "in": "收入"}.get(b["dir"], "不计收支"),
            "金额": yuan_, "收/付款方式": b["method"], "交易状态": b["status"], "交易订单号": f"2026{i:024d}"})
    x["items"].append({"id": f"{b['src']}:demo{i}", **b, "refund_fen": refund, "counted": counts, "is_refund": False, "note": "", "order": "", "kind": "", "raw": raw})
months_ = sorted(by_month, reverse=True)
cover = {s: {"first": min(b["time"] for b in bills if b["src"] == s), "last": max(b["time"] for b in bills if b["src"] == s), "export": f"{TODAY} 10:40:00",
             "count": sum(1 for b in bills if b["src"] == s)} for s in ("alipay", "wechat")}
imports_ = [{"file": f"微信支付账单流水文件({FIRST:%Y%m%d}-{TODAY:%Y%m%d})_{TODAY:%Y%m%d}104521.xlsx", "src": "wechat", "format": "微信", "export": f"{TODAY} 10:45:21", "imported_at": f"{TODAY} 10:50:02",
             "first": cover["wechat"]["first"], "last": cover["wechat"]["last"], "rows": cover["wechat"]["count"], "added": cover["wechat"]["count"], "updated": 0,
             "meta": ["微信昵称：[小舟]", f"起始时间：[{FIRST} 00:00:00] 终止时间：[{TODAY} 10:45:21]", "导出类型：[全部]", f"导出时间：[{TODAY} 10:45:21]", f"共{cover['wechat']['count']}笔记录"]},
            {"file": f"支付宝交易明细({FIRST:%Y%m%d}-{TODAY:%Y%m%d}).csv", "src": "alipay", "format": "支付宝", "export": f"{TODAY} 10:40:00", "imported_at": f"{TODAY} 10:50:02",
             "first": cover["alipay"]["first"], "last": cover["alipay"]["last"], "rows": cover["alipay"]["count"], "added": cover["alipay"]["count"], "updated": 0,
             "meta": ["姓名：小舟", f"起始时间：[{FIRST} 00:00:00] 终止时间：[{TODAY} 23:59:59]", "导出交易类型：[全部]", f"导出时间：[{TODAY} 10:40:00]", f"共{cover['alipay']['count']}笔记录"]}]
mock["get_bills"] = {m: {**x, "months": months_, "coverage": cover, "imports": imports_, "days": [{"date": d, "spend_fen": v[0], "income_fen": v[1]} for d, v in sorted(x["days"].items())],
                         "to": [{"name": n, "spend_fen": v[0], "count": v[1]} for n, v in sorted(x["to"].items(), key=lambda kv: -kv[1][0])][:10]} for m, x in by_month.items()}

for k, v in mock.items():
    (HERE / f"{k}.json").write_text(json.dumps(v, ensure_ascii=False), encoding="utf-8")
(HERE / "mock.js").write_text("window.BEAYY_MOCK = " + json.dumps(mock, ensure_ascii=False) + ";\n", encoding="utf-8")
print("写好", len(mock), "个样例，", len(days), "天，", FIRST, "～", TODAY)

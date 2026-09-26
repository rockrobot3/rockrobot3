/* 内嵌面板页面（单文件 HTML+CSS+JS，无外部资源） */
#ifndef PANEL_HTML_H
#define PANEL_HTML_H

#define PANEL_HTML \
"<!DOCTYPE html><html lang=zh><head><meta charset=utf-8>" \
"<meta name=viewport content='width=device-width,initial-scale=1'>" \
"<title>MakeRobot 面板</title><style>" \
":root{--bg:#0f172a;--card:#1e293b;--fg:#e2e8f0;--acc:#38bdf8;--ok:#4ade80;--warn:#fbbf24;--bad:#f87171}" \
"*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font-family:system-ui,sans-serif}" \
"h1{font-size:17px;padding:12px 16px;margin:0;background:var(--card)}" \
".tabs{display:flex;background:var(--card)}" \
".tabs button{flex:1;padding:10px 0;background:none;border:0;border-bottom:2px solid transparent;color:#94a3b8;font-size:14px}" \
".tabs button.on{color:var(--acc);border-bottom-color:var(--acc)}" \
".page{padding:12px;display:none}.page.on{display:block}" \
".card{background:var(--card);border-radius:10px;padding:12px 14px;margin-bottom:10px}" \
".kv{display:flex;justify-content:space-between;padding:6px 0;font-size:14px}" \
"button.act{display:block;width:100%;margin:8px 0;padding:12px;border:0;border-radius:8px;background:var(--acc);color:#0f172a;font-size:15px;font-weight:600}" \
"button.act.warn{background:var(--warn)}button.act.bad{background:var(--bad);color:#fff}" \
"input{width:100%;padding:10px;margin:6px 0;border:0;border-radius:8px;background:#0f172a;color:var(--fg);font-size:15px}" \
".wifi{display:flex;justify-content:space-between;padding:10px;border-radius:8px;background:#0f172a;margin:6px 0;font-size:14px}" \
"#msg{font-size:13px;color:var(--warn);min-height:18px;padding:2px 0}" \
".ok{color:var(--ok)}.bad{color:var(--bad)}.dim{color:#94a3b8;font-size:12px}" \
"</style></head><body>" \
"<h1>&#129302; MakeRobot 操作面板</h1>" \
"<div class=tabs>" \
"<button class=on onclick='tab(0,this)'>状态</button><button onclick='tab(1,this)'>WiFi</button>" \
"<button onclick='tab(2,this)'>硬件</button><button onclick='tab(3,this)'>系统</button></div>" \
"<div class='page on' id=p0><div class=card id=st></div>" \
"<button class=act onclick='selftest()'>运行硬件自检</button>" \
"<pre id=selfres style='font-size:13px;white-space:pre-wrap;margin:4px 0'></pre></div>" \
"<div class=page id=p1>" \
"<button class=act onclick='scan()'>扫描附近 WiFi</button><div id=list></div>" \
"<div class=card><input id=ssid placeholder='WiFi 名称（区分大小写，仅2.4G）'>" \
"<input id=pass type=password placeholder='WiFi 密码'>" \
"<button class=act onclick='connect()'>连接</button><div id=msg></div>" \
"<p class=dim>连接成功后如 IP 变化，可用新 IP 访问面板，或继续连本机热点 MakeRobot 访问 192.168.4.1</p></div></div>" \
"<div class=page id=p2>" \
"<div class=card><div class=kv><b>继电器（灯开关）</b><span id=rel>—</span></div>" \
"<button class=act onclick='relay(1)'>开灯（吸合）</button>" \
"<button class=act onclick='relay(0)'>关灯（断开）</button></div>" \
"<div class=card><div class=kv><b>喇叭 / 蜂鸣</b></div>" \
"<button class=act onclick='fetch(\"/api/beep\",{method:\"POST\"})'>播放测试音</button></div></div>" \
"<div class=page id=p3><div class=card>" \
"<button class='act bad' onclick='reboot()'>重启设备</button>" \
"<button class='act warn' onclick='factory()'>恢复出厂 WiFi</button>" \
"<p class=dim>恢复出厂后回到程序内置的默认 WiFi 账号并自动重启</p></div></div>" \
"<script>" \
"function tab(i,b){for(let k=0;k<4;k++){document.getElementById('p'+k).className='page'+(k==i?' on':'');" \
"document.querySelectorAll('.tabs button')[k].className=k==i?'on':''}if(i==0)status()}" \
"function fmt(s){s|=0;let d=s/86400|0,h=s%86400/3600|0,m=s%3600/60|0;" \
"return (d?d+' 天 ':'')+h+' 小时 '+m+' 分 '+(s%60)+' 秒'}" \
"async function status(){try{let j=await(await fetch('/api/status')).json();" \
"document.getElementById('st').innerHTML=" \
"row('连接状态',j.wifi?'<span class=ok>● 已连网</span>':'<span class=bad>● 未连网</span>')+" \
"row('WiFi 名称',j.ssid||'—')+row('IP 地址',j.ip||'—')+row('信号强度',j.rssi?j.rssi+' dBm':'—')+" \
"row('运行时长',fmt(j.uptime))+row('空闲内存',(j.heap/1024).toFixed(0)+' KB')+" \
"row('热点在线设备',j.ap_clients+' 台')+row('继电器',j.relay?'<span class=ok>吸合（开灯）</span>':'断开')}" \
"catch(e){document.getElementById('st').innerHTML='<span class=bad>面板无响应</span>'}}" \
"function row(k,v){return '<div class=kv><b>'+k+'</b><span>'+v+'</span></div>'}" \
"async function scan(){document.getElementById('list').innerHTML='<p class=dim>扫描中，约2~3秒…</p>';" \
"try{let r=await fetch('/api/wifi/scan');let j=await r.json();" \
"if(!j.length){document.getElementById('list').innerHTML='<p class=dim>没扫到网络</p>';return}" \
"document.getElementById('list').innerHTML=j.map(w=>" \
"'<div class=wifi onclick=\"document.getElementById(\\'ssid\\').value=this.dataset.s;document.getElementById(\\'pass\\').focus()\" data-s=\"'+w.ssid+'\">'" \
"+'<b>'+w.ssid+'</b><span class=dim>'+(w.auth?'&#128274; ':'')+w.rssi+' dBm</span></div>').join('')}" \
"catch(e){document.getElementById('list').innerHTML='<p class=bad>扫描失败</p>'}}" \
"async function connect(){let s=document.getElementById('ssid').value.trim();" \
"let p=document.getElementById('pass').value;let m=document.getElementById('msg');" \
"if(!s){m.textContent='请输入 WiFi 名称';return}" \
"m.textContent='连接中…（成功会有提示音）';" \
"try{let r=await(await fetch('/api/wifi/connect',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({ssid:s,pass:p})})).json();" \
"if(!r.ok){m.textContent='失败：'+r.err;return}" \
"m.textContent='已发起连接，稍等几秒后回到「状态」页查看结果';setTimeout(status,5000)}" \
"catch(e){m.textContent='请求失败'}}" \
"async function relay(v){await fetch('/api/relay',{method:'POST',headers:{'Content-Type':'application/json'},body:'{\"on\":'+v+'}'});status()}" \
"async function selftest(){document.getElementById('selfres').textContent='自检中…';" \
"try{let j=await(await fetch('/api/selftest',{method:'POST'})).json();" \
"document.getElementById('selfres').textContent=" \
"『喇叭』+(j.tone?'正常 ✓':'异常')+'\\n『继电器』'+(j.relay?'开合正常 ✓':'异常')+" \
"'\\n『麦克风』'+(j.mic_read?'能读数据 ✓':'读不到 ✘')+'（峰值 '+j.mic_peak+'，>100 表示拾音正常）'}" \
"catch(e){document.getElementById('selfres').textContent='自检失败'}}" \
"async function reboot(){if(!confirm('确定重启设备？'))return;await fetch('/api/reboot',{method:'POST'});" \
"alert('设备重启中，请约 5 秒后刷新页面')}" \
"async function factory(){if(!confirm('确定恢复出厂 WiFi？将清除面板保存的账号'))return;" \
"await fetch('/api/factory_reset',{method:'POST'});alert('已恢复出厂并重启，5 秒后刷新页面')}" \
"status();setInterval(status,10000)" \
"</script></body></html>"

#endif

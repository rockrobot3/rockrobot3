/* 内嵌面板页面（单文件 HTML+CSS+JS，无外部资源）
 * ⚠ 改动 <script> 后必须提取出来做 JS 语法校验（历史踩坑：一处漏引号全页面按钮失效） */
#ifndef PANEL_HTML_H
#define PANEL_HTML_H

#define PANEL_HTML \
"<!DOCTYPE html><html lang=zh><head><meta charset=utf-8>" \
"<meta name=viewport content='width=device-width,initial-scale=1'>" \
"<title>RockRobot3 面板</title><style>" \
":root{--bg:#0f172a;--card:#1e293b;--fg:#e2e8f0;--acc:#38bdf8;--ok:#4ade80;--warn:#fbbf24;--bad:#f87171}" \
"*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font-family:system-ui,sans-serif}" \
"h1{font-size:17px;padding:12px 16px;margin:0;background:var(--card)}" \
".tabs{display:flex;background:var(--card)}" \
".tabs button{flex:1;padding:10px 0;background:none;border:0;border-bottom:2px solid transparent;color:#94a3b8;font-size:13px}" \
".tabs button.on{color:var(--acc);border-bottom-color:var(--acc)}" \
".page{padding:12px;display:none}.page.on{display:block}" \
".card{background:var(--card);border-radius:10px;padding:12px 14px;margin-bottom:10px}" \
".kv{display:flex;justify-content:space-between;padding:6px 0;font-size:14px}" \
"button.act{display:block;width:100%;margin:8px 0;padding:12px;border:0;border-radius:8px;background:var(--acc);color:#0f172a;font-size:15px;font-weight:600}" \
"button.act.warn{background:var(--warn)}button.act.bad{background:var(--bad);color:#fff}" \
"input[type=text],input[type=password]{width:100%;padding:10px;margin:6px 0;border:0;border-radius:8px;background:#0f172a;color:var(--fg);font-size:15px}" \
".wifi{display:flex;justify-content:space-between;padding:10px;border-radius:8px;background:#0f172a;margin:6px 0;font-size:14px}" \
"#msg{font-size:13px;color:var(--warn);min-height:18px;padding:2px 0}" \
".ok{color:var(--ok)}.bad{color:var(--bad)}.dim{color:#94a3b8;font-size:12px}" \
"pre{background:#0f172a;padding:8px;border-radius:8px;font-size:12px;white-space:pre-wrap;max-height:60vh;overflow:auto}" \
"</style></head><body>" \
"<h1>&#129302; RockRobot3（马克3号）操作面板</h1>" \
"<div class=tabs>" \
"<button class=on onclick='tab(0,this)'>状态</button><button onclick='tab(1,this)'>WiFi</button>" \
"<button onclick='tab(2,this)'>硬件</button><button onclick='tab(3,this)'>日志</button>" \
"<button onclick='tab(4,this)'>系统</button></div>" \
"<div class='page on' id=p0><div class=card id=st></div>" \
"<p class=dim>版本号即固件版本（git 自动生成），排查问题先报它</p></div>" \
"<div class=page id=p1>" \
"<button class=act onclick='scan()'>扫描附近 WiFi</button><div id=list></div>" \
"<div class=card><input id=ssid type=text placeholder='WiFi 名称（区分大小写，仅2.4G）'>" \
"<input id=pass type=password placeholder='WiFi 密码'>" \
"<button class=act onclick='connect()'>连接</button><div id=msg></div>" \
"<p class=dim>提示：热点提供者（如手机）连上 RockRobot3 面板热点后，它自己的热点会暂时关闭。若目标是本手机热点：点连接后直接离开本面板，设备会自动连上它；之后可用面板显示的设备 IP 访问面板</p></div></div>" \
"<div class=page id=p2>" \
"<div class=card><div class=kv><b>继电器（灯开关）</b><span id=rel>—</span></div>" \
"<button class=act onclick='relay(1)'>开灯（吸合）</button>" \
"<button class=act onclick='relay(0)'>关灯（断开）</button></div>" \
"<div class=card><div class=kv><b>麦克风</b><span id=micst>—</span></div>" \
"<button class=act id=micbtn onclick='micToggle()'>关闭麦克风</button>" \
"<p class=dim>静音后设备完全不听不录，语音助手暂停；再点一次恢复</p></div>" \
"<div class=card><div class=kv><b>喇叭</b></div>" \
"<button class=act onclick='beep()'>播放测试音</button></div>" \
"<div class=card><div class=kv><b>单项自检</b></div>" \
"<button class=act onclick='selftestOne(\"speaker\")'>测喇叭</button>" \
"<button class=act onclick='selftestOne(\"relay\")'>测继电器</button>" \
"<button class=act onclick='selftestOne(\"mic\")'>测麦克风</button>" \
"<pre id=selfres></pre></div></div>" \
"<div class=page id=p3><div class=card>" \
"<label style='display:flex;align-items:center;gap:8px;font-size:14px'>" \
"<input type=checkbox id=auto checked style='width:auto'> 自动刷新（每 2 秒）</label>" \
"<button class=act onclick='logs()'>立即刷新</button>" \
"<pre id=logs>加载中…</pre>" \
"<p class=dim>实时交互流：对话内容、自检动作、错误全在这。最新的在最上面</p></div></div>" \
"<div class=page id=p4><div class=card><div class=kv><b>运行数据（断电保留）</b></div>" \
"<div id=data>加载中…</div>" \
"<p class=dim>失败次数突然增多=网络/云端不稳；开机次数暴涨=设备在反复重启</p></div>" \
"<div class=card>" \
"<button class='act bad' onclick='reboot()'>重启设备</button>" \
"<button class='act warn' onclick='factory()'>恢复出厂 WiFi</button>" \
"<p class=dim>恢复出厂后回到程序内置的默认 WiFi 账号并自动重启</p></div></div>" \
"<script>" \
"var sMic=true;" \
"function tab(i,b){for(var k=0;k<5;k++){document.getElementById('p'+k).className='page'+(k==i?' on':'');" \
"document.querySelectorAll('.tabs button')[k].className=k==i?'on':''}" \
"if(i==0)status();if(i==3)logs();if(i==4)dataf()}" \
"function fmt(s){s|=0;var d=s/86400|0,h=s%86400/3600|0,m=s%3600/60|0;" \
"return (d?d+' 天 ':'')+h+' 小时 '+m+' 分 '+(s%60)+' 秒'}" \
"function row(k,v){return '<div class=kv><b>'+k+'</b><span>'+v+'</span></div>'}" \
"async function status(){try{var j=await(await fetch('/api/status')).json();sMic=j.mic;" \
"document.getElementById('st').innerHTML=" \
"row('固件版本','<b>'+j.ver+'</b>')+" \
"row('连接状态',j.wifi?'<span class=ok>● 已连网</span>':'<span class=bad>● 未连网</span>')+" \
"row('WiFi 名称',j.ssid||'—')+row('IP 地址',j.ip||'—')+row('信号强度',j.rssi?j.rssi+' dBm':'—')+" \
"row('运行时长',fmt(j.uptime))+row('空闲内存',(j.heap/1024).toFixed(0)+' KB')+" \
"row('热点在线设备',j.ap_clients+' 台')+row('继电器',j.relay?'<span class=ok>吸合（开灯）</span>':'断开')+" \
"row('麦克风',j.mic?'<span class=ok>开启</span>':'<span class=bad>静音</span>');micText()}" \
"catch(e){document.getElementById('st').innerHTML='<span class=bad>面板无响应</span>'}}" \
"async function scan(){document.getElementById('list').innerHTML='<p class=dim>扫描中，约2~3秒…</p>';" \
"try{var r=await fetch('/api/wifi/scan');var j=await r.json();" \
"if(!j.length){document.getElementById('list').innerHTML='<p class=dim>没扫到网络</p>';return}" \
"document.getElementById('list').innerHTML=j.map(w=>" \
"'<div class=wifi onclick=\"document.getElementById(\\'ssid\\').value=this.dataset.s;document.getElementById(\\'pass\\').focus()\" data-s=\"'+w.ssid+'\">'" \
"+'<b>'+w.ssid+'</b><span class=dim>'+(w.auth?'&#128274; ':'')+w.rssi+' dBm</span></div>').join('')}" \
"catch(e){document.getElementById('list').innerHTML='<p class=bad>扫描失败</p>'}}" \
"async function connect(){var s=document.getElementById('ssid').value.trim();" \
"var p=document.getElementById('pass').value;var m=document.getElementById('msg');" \
"if(!s){m.textContent='请输入 WiFi 名称';return}" \
"m.textContent='连接中…（成功会有提示音）';" \
"try{var r=await(await fetch('/api/wifi/connect',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({ssid:s,pass:p})})).json();" \
"if(!r.ok){m.textContent='失败：'+r.err;return}" \
"m.textContent='已发起连接，稍等几秒后回到「状态」页查看结果';setTimeout(status,5000)}" \
"catch(e){m.textContent='请求失败'}}" \
"async function relay(v){await fetch('/api/relay',{method:'POST',headers:{'Content-Type':'application/json'},body:'{\"on\":'+v+'}'});status()}" \
"function micText(){document.getElementById('micst').innerHTML=sMic?'<span class=ok>开启</span>':'<span class=bad>静音</span>';micBtn()}" \
"function micBtn(){document.getElementById('micbtn').textContent=sMic?'关闭麦克风（设备闭嘴）':'打开麦克风'}" \
"async function micToggle(){try{var j=await(await fetch('/api/mic',{method:'POST',headers:{'Content-Type':'application/json'},body:'{\"on\":'+!sMic+'}'})).json();sMic=j.mic;micText()}catch(e){}}" \
"async function beep(){await fetch('/api/beep',{method:'POST'})}" \
"async function selftestOne(item){var el=document.getElementById('selfres');el.textContent='自检中…';" \
"try{var j=await(await fetch('/api/selftest',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({item:item})})).json();" \
"var t={speaker:'『喇叭』已播放测试音，请确认听到声音'," \
"relay:'『继电器』开合一次完成，请确认听到咔哒声'," \
"mic:'『麦克风』'+(j.mic_read?'能读数据 ✓':'读不到 ✘')+'（峰值 '+j.mic_peak+'，>100 表示拾音正常）'}[item];" \
"el.textContent=t}catch(e){el.textContent='自检失败'}}" \
"async function logs(){try{var j=await(await fetch('/api/logs?n=30')).json();" \
"document.getElementById('logs').textContent=j.join('\\n')}catch(e){}}" \
"async function dataf(){try{var j=await(await fetch('/api/data')).json();" \
"var lb={boots:'开机次数',chat_rounds:'对话轮数',asr_ok:'识别成功',asr_fail:'识别失败'," \
"llm_fail:'AI 回答失败',tts_fail:'合成失败',relay_on:'继电器吸合',relay_off:'继电器断开',errors:'其他错误'};" \
"var h='';for(var k in j)h+=row(lb[k]||k,j[k]);" \
"document.getElementById('data').innerHTML=h}catch(e){}}" \
"async function reboot(){if(!confirm('确定重启设备？'))return;await fetch('/api/reboot',{method:'POST'});" \
"alert('设备重启中，几秒后重新连上')}" \
"async function factory(){if(!confirm('恢复出厂将清除已保存的 WiFi 并重启，确定？'))return;" \
"await fetch('/api/factory_reset',{method:'POST'});alert('已恢复出厂，设备重启中')}" \
"setInterval(function(){var p=document.getElementById('p3');" \
"if(p.className.indexOf('on')>=0&&document.getElementById('auto').checked)logs()},2000);" \
"status()" \
"</script></body></html>"

#endif

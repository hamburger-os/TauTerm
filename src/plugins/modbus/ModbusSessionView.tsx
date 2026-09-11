import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "../../context/SessionContext";
import styles from "./Modbus.module.css";
import { FUNCTION_LABELS, hex, parseHex, traditionalAddress, type ModbusRequest, type ServerSnapshot, type TransactionResult, type WatchRow, type WatchValue } from "./model";

type Page = "readwrite" | "monitor" | "transactions" | "advanced" | "server";

export default function ModbusSessionView({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const role = String(tab?.params?.role ?? "client");
  const mode = String(tab?.params?.mode ?? "tcp");
  const unit = Number(tab?.params?.unit_id ?? 1);
  const [page, setPage] = useState<Page>(role === "server" ? "server" : "readwrite");
  const [history, setHistory] = useState<TransactionResult[]>([]);
  const execute = async (request: ModbusRequest) => {
    const result = await invoke<TransactionResult>("modbus_execute", { sessionId, request });
    setHistory(current => [result, ...current].slice(0, 500));
    return result;
  };
  const pages: [Page, string][] = role === "server"
    ? [["server", "Server"], ["transactions", "事务"], ["advanced", "高级"]]
    : [["readwrite", "读写"], ["monitor", "监控"], ["transactions", "事务"], ["advanced", "高级"]];
  return <div className={styles.root}>
    <div className={styles.header}>
      <span className={styles.title}>Modbus</span><span className={styles.badge}>{mode.toUpperCase()}</span><span className={styles.badge}>{role === "server" ? "Server" : "Client"}</span>
      <span className={styles.meta}>{tab?.endpoint || (mode === "tcp" ? `${String(tab?.params?.host ?? "")}:${String(tab?.params?.port ?? 502)}` : String(tab?.params?.serial_port ?? ""))} · Unit {unit}</span>
    </div>
    <div className={styles.tabs}>{pages.map(([id,label]) => <button key={id} className={`${styles.tab} ${page===id?styles.tabActive:""}`} onClick={()=>setPage(id)}>{label}</button>)}</div>
    <div className={styles.body}>
      {page === "readwrite" && <ReadWrite execute={execute} />}
      {page === "monitor" && <Monitor sessionId={sessionId} />}
      {page === "transactions" && <Transactions history={history} />}
      {page === "advanced" && <Advanced execute={execute} mode={mode} role={role} />}
      {page === "server" && <ServerPanel sessionId={sessionId} />}
    </div>
  </div>;
}

function ReadWrite({ execute }: { execute: (request: ModbusRequest)=>Promise<TransactionResult> }) {
  const [fc,setFc]=useState(3); const [address,setAddress]=useState(0); const [quantity,setQuantity]=useState(1); const [value,setValue]=useState("0"); const [result,setResult]=useState<TransactionResult|null>(null); const [busy,setBusy]=useState(false); const [error,setError]=useState("");
  const isRead=[1,2,3,4].includes(fc); const isMulti=[15,16].includes(fc);
  const run=async()=>{setBusy(true);setError("");try{let request:ModbusRequest;if(fc===1||fc===2)request={kind:"read_bits",function:fc as 1|2,address,quantity};else if(fc===3||fc===4)request={kind:"read_registers",function:fc as 3|4,address,quantity};else if(fc===5||fc===6)request={kind:"write_single",function:fc as 5|6,address,value:fc===5?(Number(value)!==0?0xff00:0):Number(value)};else if(fc===15){const bits=value.split(/[\s,;]+/).filter(Boolean).map(v=>v==="1"||v.toLowerCase()==="true");const bytes=new Array(Math.ceil(bits.length/8)).fill(0);bits.forEach((bit,i)=>{if(bit)bytes[Math.floor(i/8)]|=1<<(i%8)});request={kind:"write_multiple_coils",address,quantity:bits.length,values:bytes};}else{request={kind:"write_multiple_registers",address,values:value.split(/[\s,;]+/).filter(Boolean).map(Number)};}setResult(await execute(request));}catch(e){setError(String(e));}finally{setBusy(false)}};
  return <div className={`${styles.card} liquid-glass-card`}>
    <div className={styles.grid}>
      <label className={styles.field}><span className={styles.label}>功能</span><select className="liquid-glass-input liquid-glass-select" value={fc} onChange={e=>setFc(Number(e.target.value))}>{[1,2,3,4,5,6,15,16].map(code=><option key={code} value={code}>{FUNCTION_LABELS[code]}</option>)}</select></label>
      <label className={styles.field}><span className={styles.label}>协议地址 (0-based)</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={e=>setAddress(Number(e.target.value))}/><span className={styles.hint}>传统引用：{traditionalAddress(fc,address)}</span></label>
      {isRead && <label className={styles.field}><span className={styles.label}>数量</span><input className="liquid-glass-input" type="number" min={1} max={fc<=2?2000:125} value={quantity} onChange={e=>setQuantity(Number(e.target.value))}/></label>}
    </div>
    {!isRead && <label className={styles.field}><span className={styles.label}>{isMulti?"值列表":"值"}</span><textarea className={`${styles.textarea} liquid-glass-input`} value={value} onChange={e=>setValue(e.target.value)} placeholder={fc===15?"1 0 1 1":"10 20 30"}/></label>}
    <div className={styles.actions}><button className={`${styles.button} liquid-glass-button`} disabled={busy} onClick={()=>void run()}>{busy?"执行中…":"执行"}</button>{error&&<span className={styles.error}>{error}</span>}</div>
    {result&&<ResultCard result={result}/>} 
  </div>;
}

function ResultCard({result}:{result:TransactionResult}){const cls=result.status==="success"||result.status==="broadcast"?styles.success:result.status==="timeout"?styles.warning:styles.error;return <div className={styles.notice}><strong className={cls}>{result.status}</strong> · {result.latency_ms} ms{result.write_outcome_unknown&&<span className={styles.warning}> · 响应超时，写入结果未知</span>}<div className={styles.mono}>TX {hex(result.raw_tx)}</div>{result.raw_rx.length>0&&<div className={styles.mono}>RX {hex(result.raw_rx)}</div>}{result.exception_code!=null&&<div>Exception 0x{result.exception_code.toString(16).padStart(2,"0").toUpperCase()}</div>}{result.message&&<div>{result.message}</div>}</div>}

function Monitor({sessionId}:{sessionId:string}){
  const [rows,setRows]=useState<WatchRow[]>([{id:crypto.randomUUID(),enabled:true,name:"Holding 0",request:{kind:"read_registers",function:3,address:0,quantity:1},period_ms:1000,format:{value_type:"uint16",byte_order:"ABCD",scale:1,offset:0,unit:""}}]);
  const [values,setValues]=useState<Record<string,WatchValue>>({});const [running,setRunning]=useState(false);const [error,setError]=useState("");
  useEffect(()=>{if(!running)return;const timer=window.setInterval(()=>{void invoke<WatchValue[]>("modbus_watch_values",{sessionId}).then(items=>setValues(Object.fromEntries(items.map(item=>[item.row_id,item])))).catch(()=>{})},300);return()=>window.clearInterval(timer)},[running,sessionId]);
  const apply=async(start:boolean)=>{setError("");try{await invoke("modbus_watch_set",{sessionId,rows});await invoke(start?"modbus_watch_start":"modbus_watch_stop",{sessionId});setRunning(start);}catch(e){setError(String(e))}};
  const add=()=>setRows(current=>[...current,{id:crypto.randomUUID(),enabled:true,name:`Watch ${current.length+1}`,request:{kind:"read_registers",function:3,address:0,quantity:1},period_ms:1000,format:{value_type:"uint16",byte_order:"ABCD",scale:1,offset:0,unit:""}}]);
  return <div className={`${styles.card} liquid-glass-card`}><div className={styles.actions}><button className="liquid-glass-button" onClick={add}>添加</button><button className="liquid-glass-button" onClick={()=>void apply(!running)}>{running?"停止轮询":"开始轮询"}</button>{error&&<span className={styles.error}>{error}</span>}</div><div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>启用</th><th>名称</th><th>功能</th><th>地址</th><th>数量</th><th>周期 ms</th><th>当前值</th><th>状态</th><th/></tr></thead><tbody>{rows.map(row=>{const req=row.request.kind==="read_registers"?row.request:null;const current=values[row.id];return <tr key={row.id}><td><input type="checkbox" checked={row.enabled} onChange={e=>setRows(list=>list.map(item=>item.id===row.id?{...item,enabled:e.target.checked}:item))}/></td><td><input className="liquid-glass-input" value={row.name} onChange={e=>setRows(list=>list.map(item=>item.id===row.id?{...item,name:e.target.value}:item))}/></td><td>03</td><td><input className="liquid-glass-input" type="number" value={req?.address??0} onChange={e=>setRows(list=>list.map(item=>item.id===row.id?{...item,request:{kind:"read_registers",function:3,address:Number(e.target.value),quantity:req?.quantity??1}}:item))}/></td><td><input className="liquid-glass-input" type="number" min={1} max={125} value={req?.quantity??1} onChange={e=>setRows(list=>list.map(item=>item.id===row.id?{...item,request:{kind:"read_registers",function:3,address:req?.address??0,quantity:Number(e.target.value)}}:item))}/></td><td><input className="liquid-glass-input" type="number" min={20} value={row.period_ms} onChange={e=>setRows(list=>list.map(item=>item.id===row.id?{...item,period_ms:Number(e.target.value)}:item))}/></td><td className={styles.mono}>{current?.value==null?"—":String(current.value)}</td><td>{current?.status??"—"}</td><td><button className="liquid-glass-button" onClick={()=>setRows(list=>list.filter(item=>item.id!==row.id))}>×</button></td></tr>})}</tbody></table></div></div>;
}

function Transactions({history}:{history:TransactionResult[]}){return <div className={`${styles.card} liquid-glass-card`}><div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>结果</th><th>Unit</th><th>功能</th><th>TID</th><th>耗时</th><th>TX / RX</th></tr></thead><tbody>{history.length===0?<tr><td colSpan={6} className={styles.empty}>尚无事务</td></tr>:history.map((item,index)=><tr key={`${item.transaction_id}-${index}`}><td>{item.status}</td><td>{item.unit_id}</td><td>0x{item.function.toString(16).padStart(2,"0").toUpperCase()}</td><td>{item.transaction_id??"—"}</td><td>{item.latency_ms} ms</td><td className={styles.mono}>TX {hex(item.raw_tx)}<br/>RX {hex(item.raw_rx)}</td></tr>)}</tbody></table></div></div>}

function Advanced({execute,mode,role}:{execute:(request:ModbusRequest)=>Promise<TransactionResult>;mode:string;role:string}){
  const [fc,setFc]=useState("2B");const [data,setData]=useState("0E 01 00");const [result,setResult]=useState<TransactionResult|null>(null);const [error,setError]=useState("");
  const run=async()=>{try{setError("");const functionCode=Number.parseInt(fc,16);const bytes=parseHex(data);setResult(await execute({kind:"raw",function:functionCode,data:bytes}));}catch(e){setError(String(e))}};
  if(role==="server")return <div className={`${styles.card} liquid-glass-card`}><strong>Server 故障注入</strong><p className={styles.hint}>连接参数中可配置延迟、无响应或标准异常；数据区可在 Server 页实时编辑。Raw ADU 故障帧与自动封装 PDU 分离，避免自动修复改变测试字节。</p></div>;
  return <div className={`${styles.card} liquid-glass-card`}><div className={styles.notice}>Raw PDU：TauTerm 自动添加 {mode==="tcp"?"MBAP + Unit ID":mode==="rtu"?"Unit ID + CRC":"Unit ID + LRC + ASCII delimiters"}。畸形 Raw ADU 不与此模式混用。</div><div className={styles.twoColumns}><label className={styles.field}><span className={styles.label}>Function Code (hex)</span><input className="liquid-glass-input" value={fc} onChange={e=>setFc(e.target.value)}/></label><label className={styles.field}><span className={styles.label}>PDU Data (hex)</span><input className="liquid-glass-input" value={data} onChange={e=>setData(e.target.value)}/></label></div><div className={styles.actions}><button className="liquid-glass-button" onClick={()=>void run()}>发送 Raw PDU</button>{error&&<span className={styles.error}>{error}</span>}</div>{result&&<ResultCard result={result}/>}</div>
}

function ServerPanel({sessionId}:{sessionId:string}){const [snapshot,setSnapshot]=useState<ServerSnapshot|null>(null);const [area,setArea]=useState("holding_register");const [address,setAddress]=useState(0);const [value,setValue]=useState(0);const refresh=()=>void invoke<ServerSnapshot>("modbus_server_snapshot",{sessionId}).then(setSnapshot);useEffect(()=>{refresh();const timer=window.setInterval(refresh,1000);return()=>window.clearInterval(timer)},[sessionId]);const set=async()=>{await invoke("modbus_server_set_value",{sessionId,area,address,value});refresh()};const list=area==="coil"?snapshot?.coils:area==="discrete_input"?snapshot?.discrete_inputs:area==="input_register"?snapshot?.input_registers:snapshot?.holding_registers;return <div className={styles.split}><div className={`${styles.card} liquid-glass-card`}><strong>数据模型</strong><label className={styles.field}><span className={styles.label}>区域</span><select className="liquid-glass-input liquid-glass-select" value={area} onChange={e=>setArea(e.target.value)}><option value="coil">Coils</option><option value="discrete_input">Discrete Inputs</option><option value="holding_register">Holding Registers</option><option value="input_register">Input Registers</option></select></label><label className={styles.field}><span className={styles.label}>协议地址</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={e=>setAddress(Number(e.target.value))}/></label><label className={styles.field}><span className={styles.label}>值</span><input className="liquid-glass-input" type="number" value={value} onChange={e=>setValue(Number(e.target.value))}/></label><button className="liquid-glass-button" onClick={()=>void set()}>写入模拟数据</button></div><div className={`${styles.card} liquid-glass-card`}><strong>当前区域</strong><div className={styles.tableWrap}><table className={styles.table}><thead><tr><th>地址</th><th>值</th></tr></thead><tbody>{(list??[]).map(([a,v])=><tr key={a}><td>{a}</td><td>{String(v)}</td></tr>)}</tbody></table></div></div></div>}

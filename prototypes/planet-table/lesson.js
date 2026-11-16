/* A seekable, deterministic SVG film. All motion belongs to one GSAP timeline. */
'use strict';
const svg = document.querySelector('#stage');
const C = {mint:'#85edca',blue:'#82b6ff',amber:'#ffc580',pink:'#e6a2cf',line:'#334b65'};
const chapters = [
  [0,18,'The expensive question'],[18,38,'Start with 2D'],[38,59,'Between the entries'],
  [59,78,'Add a third axis'],[78,99,'Then a fourth'],[99,122,'A real planet'],
  [122,144,'Inside Night Sky'],[144,164,'What tables do not do']
];
const text = (x,y,s,cls='',extra='') => `<text x="${x}" y="${y}" class="${cls}" ${extra}>${s}</text>`;
const rect = (x,y,w,h,cls='panel',extra='') => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="14" class="${cls}" ${extra}/>`;
const line = (x,y,a,b,color=C.line,extra='') => `<line x1="${x}" y1="${y}" x2="${a}" y2="${b}" stroke="${color}" stroke-width="2" ${extra}/>`;
const dot = (x,y,r=6,color=C.mint,extra='') => `<circle cx="${x}" cy="${y}" r="${r}" fill="${color}" ${extra}/>`;
const arrow = (x,y,a,b,color=C.mint) => line(x,y,a,b,color,'marker-end="url(#arrow)"');
const group = (id,content,cls='') => `<g id="${id}" class="${cls}">${content}</g>`;
const heading = (n,title,sub) => text(80,66,`FIELD NOTE 01 / ${n}`, 'overline')+text(80,140,title,'title')+text(80,188,sub,'subtitle');
const caption = (a,b='') => line(80,767,1520,767,'#26384d')+text(80,815,a,'caption')+text(80,852,b,'small muted');
function planet(x,y,r,id='planet') {
  return group(id,`<circle cx="${x}" cy="${y}" r="${r+12}" fill="none" stroke="${C.blue}" opacity=".13" stroke-width="12"/><circle cx="${x}" cy="${y}" r="${r}" fill="url(#planetFill)"/>`+
    `<path d="M ${x} ${y-r} A ${r} ${r} 0 0 1 ${x} ${y+r} Z" fill="#5b8db3" opacity=".4"/>`+
    `<circle cx="${x}" cy="${y}" r="${r*.69}" fill="#ca9977"/><circle cx="${x}" cy="${y}" r="${r*.34}" fill="#ffd4a0"/>`);
}
function grid(x,y,step=100,cols=5,rows=4,values=false) {
  let s='';
  for(let i=0;i<cols;i++)for(let j=0;j<rows;j++){
    s+=rect(x+i*step,y+j*step,step-4,step-4,'',`fill="${['#173b45','#1b4352','#244d60','#2c576c','#35627a'][(i+j)%5]}" rx="4"`);
    if(values)s+=text(x+i*step+step/2,y+j*step+step/2+6,(.7+i*.18-j*.045).toFixed(2),'small mono','text-anchor="middle"');
  }
  return s;
}
function lattice(x,y,z,color=C.blue){
  let s='';const p=(i,j,k)=>[x+i*105+k*39,y-j*90-k*30];
  for(let k=0;k<3;k++)for(let j=0;j<3;j++)for(let i=0;i<3;i++){
    let [a,b]=p(i,j,k);
    if(i<2){let[q,r]=p(i+1,j,k);s+=line(a,b,q,r,color,'opacity=".35"');}
    if(j<2){let[q,r]=p(i,j+1,k);s+=line(a,b,q,r,color,'opacity=".35"');}
    if(k<2){let[q,r]=p(i,j,k+1);s+=line(a,b,q,r,color,'opacity=".35"');}
    s+=dot(a,b,4,color);
  }
  // Highlight the eight knots of one cell, not every knot in the whole grid.
  const face=[[0,0,0],[1,0,0],[1,1,0],[0,1,0]].map(v=>p(...v).join(',')).join(' ');
  s+=`<polygon points="${face}" fill="${C.mint}" opacity=".13"/>`;
  for(let k=0;k<2;k++)for(let j=0;j<2;j++)for(let i=0;i<2;i++){
    const [a,b]=p(i,j,k);s+=dot(a,b,6,C.mint);
  }
  const [qx,qy]=p(.6,.4,.6);
  s+=dot(qx,qy,8,C.mint)+text(x+10,y+37,'8 selected knots','tiny mint');
  return `<g transform="translate(${z||0},0)">${s}</g>`;
}
function scene0(){return heading('01','A planet, without the wait.','How Night Sky turns expensive physics into a fast lookup.')+
  planet(350,455,153,'intro-planet')+
  group('solver',rect(650,279,790,339)+text(690,327,'THE DIRECT APPROACH','overline')+
  text(690,386,'Guess the central pressure.','')+text(690,432,'Integrate outward through the layers.','')+
  text(690,478,'Check the mass and boundary.','')+text(690,524,'Adjust. Repeat.','mint')+
  `<path id="solver-loop" d="M 1360 370 C 1460 370 1460 550 1350 550" fill="none" stroke="${C.amber}" stroke-width="3" marker-end="url(#arrow)"/>`)+
  group('offline',rect(650,650,790,66)+text(684,692,'Instead: solve a grid of examples before shipping the game.','small mint'))+
  caption('The solver is useful. Repeating all its work for every new planet is costly.','The table stores its answers so generation can reuse them. Diagram is schematic, not a timing benchmark.');}
function scene1(){return heading('02','Start with two inputs.','Imagine every planet has the same water fraction and water temperature.')+
  group('grid2',grid(210,285,100,6,4,true)+text(507,748,'Mass →','small mint','text-anchor="middle"')+
  text(150,505,'Metal share →','small blue','transform="rotate(-90 150 505)"')+
  text(210,268,'SMALLER','tiny muted')+text(710,268,'LARGER','tiny muted'))+
  group('table-key',rect(940,300,500,325)+text(975,352,'TWO AXES, ONE ANSWER','overline')+
  text(975,412,'Column: how much mass?')+text(975,461,'Row: how metal-rich?')+
  text(975,528,'Each knot stores a solved radius.','mint')+text(975,572,'Numbers here are illustrative.','small muted'))+
  group('exact-point',rect(510,485,96,96,'','fill="none" stroke="#85edca" stroke-width="4"')+dot(558,533,10)+arrow(825,531,923,531))+
  caption('A two-dimensional table is just a set of solved examples indexed by two inputs.','Metal share means metal / (metal + rock), not the fraction of the whole planet. Coloured tiles represent stored knots.');}
function scene2(){let pts=[[350,570,'6,400'],[680,570,'7,000'],[350,300,'6,250'],[680,300,'6,850']];return heading('03','Planets fall between the knots.','Do not pick the nearest answer. Blend the surrounding answers.')+
  rect(350,300,330,270,'','fill="#1c3b46" stroke="#476578" stroke-width="2"')+
  pts.map(([x,y,v],i)=>group(`corner${i}`,dot(x,y,10,C.blue)+text(x+(x===350?-22:22),y-20,v+' km','small blue',`text-anchor="${x===350?'end':'start'}"`))).join('')+
  group('mix-lines',pts.map(([x,y])=>line(x,y,548,459,C.mint,'stroke-dasharray="5 7" opacity=".6"')).join(''))+
  group('query2',dot(548,459,12)+text(570,447,'Your planet','small mint'))+
  group('corner-weights',text(365,551,'24%','tiny mint')+text(623,551,'36%','tiny mint')+text(365,332,'16%','tiny mint')+text(623,332,'24%','tiny mint'))+
  text(508,650,'Mass →','small mint','text-anchor="middle"')+text(235,480,'Metal share →','small blue','transform="rotate(-90 235 480)"')+
  group('blend-card',rect(940,292,500,355)+text(976,340,'BILINEAR INTERPOLATION','overline')+
  text(976,405,'60% along the mass axis.')+text(976,449,'40% along the metal axis.')+
  text(976,505,'4 corners → weighted blend','mint')+text(976,555,'Closer knots contribute more.','small muted')+
  text(976,603,'Night Sky blends ln(radius).','small amber'))+
  caption('Move the input smoothly, and the interpolated radius moves smoothly too.','Within a cell this is continuous, not necessarily smooth in slope across its boundaries. These four radii are illustrative.');}
function scene3(){return heading('04','A third input adds depth.','Let the water share vary. A 2D table becomes a stack of 2D slices.')+
  [0,1,2].map((k)=>group(`slice${k}`,`<g transform="translate(${240+k*110},${430-k*50}) skewY(-12)">${grid(0,0,62,5,4)}</g>`+text(240+k*110,720-k*25,`${[0,10,30][k]}% water`,'small blue'))).join('')+
  group('cube-card',rect(970,295,480,340)+text(1005,344,'THREE INPUTS','overline')+
  text(1005,405,'Mass · metal · water')+text(1005,469,'2 × 2 × 2 = 8 corners','mint')+
  text(1005,526,'Blend within each slice.','small muted')+text(1005,568,'Then blend between the slices.','small muted'))+
  caption('Each new axis describes another way planets can differ.','This is a storage dimension, not an extra physical direction in space. The visible grid is simplified.');}
function scene4(){return heading('05','Four dimensions, no impossible drawing.','Water temperature becomes a fourth input: a collection of 3D grids.')+
  group('cube-cold',lattice(255,590,0,C.blue)+text(365,654,'285 K','blue','text-anchor="middle"')+text(365,691,'lower temperature slice','small muted','text-anchor="middle"'))+
  group('cube-hot',lattice(785,590,0,C.amber)+text(895,654,'300 K','amber','text-anchor="middle"')+text(895,691,'upper temperature slice','small muted','text-anchor="middle"'))+
  group('fourth-blend',arrow(580,440,745,440)+text(660,407,'blend','small mint','text-anchor="middle"'))+
  group('dimension-count',rect(1200,300,305,335)+text(1232,351,'4D LOOKUP','overline')+text(1232,437,'16','big mint')+
  text(1232,487,'corner values')+text(1232,551,'2 × 2 × 2 × 2','small mono muted')+text(1232,596,'Not 16 planets to solve.','tiny muted'))+
  caption('Blend eight corners at the lower temperature, eight at the upper, then blend those results.','The implementation does this as one weighted sum over 16 knots. Mass is located on a logarithmic axis.');}

// Sixteen condensed-grid knots extracted from NSSTR001, 16 November 2026.
// [mass in Earth masses, metal share, water share, kelvin, radius km, inertia factor]
const knots = [
 [.9457416090031777,.25,.075,285,6459.07025,.3178189],[.9457416090031777,.25,.075,300,6463.97535,.3173464],
 [.9457416090031777,.25,.1,285,6554.76322,.3117886],[.9457416090031777,.25,.1,300,6559.82232,.3113186],
 [.9457416090031777,.375,.075,285,6322.87859,.3059723],[.9457416090031777,.375,.075,300,6327.57768,.3055265],
 [.9457416090031777,.375,.1,285,6425.1511,.3000294],[.9457416090031777,.375,.1,300,6430.01204,.2995857],
 [1.2832115105860769,.25,.075,285,7080.76137,.3164928],[1.2832115105860769,.25,.075,300,7085.12499,.3161093],
 [1.2832115105860769,.25,.1,285,7185.06186,.3105646],[1.2832115105860769,.25,.1,300,7189.55831,.310183],
 [1.2832115105860769,.375,.075,285,6930.35069,.3046075],[1.2832115105860769,.375,.075,300,6934.53564,.3042459],
 [1.2832115105860769,.375,.1,285,7041.7909,.2987782],[1.2832115105860769,.375,.1,300,7046.11034,.2984181]
];
const query=[1.1,.325,.09,290];
const bounds=[[knots[0][0],knots[8][0]],[.25,.375],[.075,.1],[285,300]];
const fractions=query.map((v,i)=>i===0?Math.log(v/bounds[i][0])/Math.log(bounds[i][1]/bounds[i][0]):(v-bounds[i][0])/(bounds[i][1]-bounds[i][0]));
const weights=knots.map(k=>k.slice(0,4).reduce((w,v,i)=>w*(v===bounds[i][1]?fractions[i]:1-fractions[i]),1));
const result={radius:Math.exp(knots.reduce((s,k,i)=>s+weights[i]*Math.log(k[4]),0)),inertia:knots.reduce((s,k,i)=>s+weights[i]*k[5],0)};
function scene5(){let entries='';for(let i=0;i<16;i++){
  const x=660+(i%4)*114,y=314+Math.floor(i/4)*78;
  entries+=group(`real-knot${i}`,rect(x,y,104,66,'',`fill="#1b3d49" stroke="#376456"`)+text(x+52,y+29,Math.round(knots[i][4]).toLocaleString('en-GB'),'small mono','text-anchor="middle"')+text(x+52,y+51,(weights[i]*100).toFixed(1)+'%','tiny mint','text-anchor="middle"'));
}
return heading('06','Follow one real lookup.','A rock, metal and water example using sixteen entries from the game’s embedded table.')+
  group('inputs',rect(80,280,460,407)+text(115,327,'INPUTS AFTER RETENTION','overline')+
  text(115,383,'Mass')+text(505,383,'1.10 M⊕','mint','text-anchor="end"')+
  text(115,435,'Metal / refractory')+text(505,435,'32.5%','blue','text-anchor="end"')+
  text(115,487,'Water / total')+text(505,487,'9.0%','blue','text-anchor="end"')+
  text(115,539,'Water temperature')+text(505,539,'290 K','amber','text-anchor="end"')+
  text(115,609,'Whole-body mass fractions:','small muted')+text(115,647,'29.575% metal · 61.425% rock · 9% water','tiny muted'))+
  group('real-corners',text(660,285,'16 STORED RADII (km) + THEIR WEIGHTS','tiny muted')+entries)+
  group('real-result',rect(1180,314,340,357)+text(1210,359,'INTERPOLATED','overline')+
  text(1210,434,Math.round(result.radius).toLocaleString('en-GB'),'big mint')+text(1210,473,'km radius')+
  text(1210,538,result.inertia.toFixed(4),'blue')+text(1210,574,'inertia factor','small muted')+
  text(1210,625,'No radial solve for this lookup.','tiny mint'))+
  arrow(555,480,635,480)+arrow(1120,480,1165,480)+
  caption('Locate a bracket on each axis. Multiply the four axis weights. Blend the stored values.','R = exp(Σ weight × ln R). Inertia = Σ weight × inertia. This is a condensed-radius lookup, not a full atmosphere calculation.');}
function scene6(){return heading('07','The table is part of generation.','Composition is an input to the lookup, not something the table invents.')+
  group('pipeline-a',rect(80,280,360,180)+text(110,325,'01 / GENERATE','overline')+text(110,380,'Mass, orbit, inventory')+text(110,421,'Retention and environment','small muted'))+
  group('pipeline-b',rect(580,280,405,180)+text(610,325,'02 / LOOK UP','overline')+text(610,380,'Choose the structural route')+text(610,421,'Blend its nearby table entries','small muted'))+
  group('pipeline-c',rect(1125,280,395,180)+text(1155,325,'03 / BUILD THE PLANET','overline')+text(1155,380,'Use the reference radius')+text(1155,421,'Derive density, gravity, shape','small muted'))+
  arrow(460,370,560,370)+arrow(1005,370,1105,370)+
  group('routes',rect(80,510,905,206)+text(110,550,'THREE SEPARATE GRIDS, NOT ONE GIANT TABLE','overline')+
  text(110,595,'Condensed: mass · metal · water · water temperature','small')+
  text(110,636,'Envelope: total mass · H/He share · equilibrium heat · internal heat','small')+
  text(110,677,'Core under gas: its own mixture, compressed by the gas above it','small muted'))+
  group('direct',rect(1125,510,395,206)+text(1155,550,'STILL CALCULATED','overline')+
  text(1155,595,'Escape, gas / frost partition,','small')+text(1155,631,'thin-atmosphere temperature','small')+
  text(1155,676,'Not all facts come from a table.','small amber'))+
  caption('The costly radial integration is replaced by a small blend, not by selecting a finished planet.','Surface decisions may repeat cheap lookups. Background layered refinement still exists; its radius is fitted to the initial radius.');}
function scene7(){return heading('08','Fast does not mean exact.','Tables trade storage and offline work for bounded runtime cost.')+
  group('storage',rect(80,280,680,200)+text(115,328,'MORE INPUTS ≠ MORE OUTPUTS','overline')+
  text(115,386,'Add a 10-knot axis: 10× as many entries.')+text(115,434,'Add one output: one more number per entry.','small mint'))+
  group('limits',rect(800,280,720,200)+text(835,328,'THE MODEL HAS LIMITS','overline')+
  text(835,386,'Interpolation has error. Unsupported cells are filled.')+text(835,434,'Filled or out-of-domain lookups are labelled approximate.','small amber'))+
  group('today',rect(80,520,680,198)+text(115,568,'TODAY','overline')+text(115,622,'Table-led size. Cheap direct atmosphere logic.')+
  text(115,668,'A background solver still refines the interior.','small muted'))+
  group('future',rect(800,520,720,198)+text(835,568,'DIRECTION, NOT COMPLETED','overline')+
  text(835,622,'Make generation the authority for visible facts.')+text(835,668,'Potentially tabulate interior detail and remove runtime solves.','small mint'))+
  caption('Compute the examples once. Interpolate the answer. Keep the planet stable.','More tables can replace more work, but only when their inputs, accuracy, coverage and consumers have been checked.');}

svg.innerHTML=`<defs><radialGradient id="planetFill" cx="30%" cy="25%"><stop stop-color="#b1deef"/><stop offset=".5" stop-color="#4f87a9"/><stop offset="1" stop-color="#1e3f5c"/></radialGradient><radialGradient id="halo"><stop stop-color="#14313a" stop-opacity=".55"/><stop offset="1" stop-color="#0b1320" stop-opacity="0"/></radialGradient><marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10" fill="#85edca"/></marker><pattern id="dots" width="32" height="32" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r="1" fill="#30415a" opacity=".35"/></pattern></defs><rect width="1600" height="900" fill="#0b1320"/><rect width="1600" height="900" fill="url(#dots)"/><ellipse cx="550" cy="380" rx="760" ry="500" fill="url(#halo)"/>`+
  [scene0,scene1,scene2,scene3,scene4,scene5,scene6,scene7].map((fn,i)=>group(`scene${i}`,fn(),'scene')).join('');
const nav=document.querySelector('#chapters');
nav.innerHTML=chapters.map(([s,e,name],i)=>`<button data-chapter="${i}"><span>0${i+1} / ${format(s)}</span>${name}</button>`).join('');
function format(s){return `${Math.floor(s/60)}:${String(Math.floor(s%60)).padStart(2,'0')}`;}
if(!window.gsap){document.querySelector('#load-error').hidden=false;}else{
  const timeline=gsap.timeline({paused:true,onUpdate:update,onComplete:update});
  chapters.forEach(([start,end],i)=>{
    timeline.set(`#scene${i}`,{autoAlpha:1},start);
    timeline.fromTo(`#scene${i} .title`,{opacity:0,y:16},{opacity:1,y:0,duration:.7},start+.1);
    timeline.fromTo(`#scene${i} .subtitle`,{opacity:0},{opacity:1,duration:.8},start+.5);
    if(i<7)timeline.set(`#scene${i}`,{autoAlpha:0},end);
  });
  const reveal=(target,time,duration=.8)=>timeline.fromTo(target,{opacity:0,y:14},{opacity:1,y:0,duration,ease:'power2.out'},time);
  reveal('#intro-planet',.3);reveal('#solver',2);reveal('#offline',11);
  timeline.fromTo('#solver-loop',{strokeDasharray:400,strokeDashoffset:400},{strokeDashoffset:0,duration:3},6);
  reveal('#grid2',19);reveal('#table-key',23);reveal('#exact-point',29);
  for(let i=0;i<4;i++)reveal(`#corner${i}`,39+i*.6);
  timeline.fromTo('#query2',{opacity:0},{opacity:1,duration:.5},42);
  reveal('#mix-lines',47);reveal('#corner-weights',48);reveal('#blend-card',47);
  timeline.fromTo('#query2',{x:-198,y:111},{x:0,y:0,duration:4,ease:'power1.inOut'},42);
  for(let i=0;i<3;i++){
    timeline.fromTo(`#slice${i}`,{opacity:0},{opacity:1,duration:.5},60+i*3);
    timeline.fromTo(`#slice${i}`,{x:-i*110,y:i*50},{x:0,y:0,duration:2,ease:'power2.inOut'},60+i*3);
  }
  reveal('#cube-card',69);reveal('#cube-cold',79);reveal('#cube-hot',84);reveal('#fourth-blend',89);reveal('#dimension-count',92);
  reveal('#inputs',100);reveal('#real-corners',104);
  for(let i=0;i<16;i++)reveal(`#real-knot${i}`,104+i*.18,.4);
  reveal('#real-result',112);
  reveal('#pipeline-a',123);reveal('#pipeline-b',127);reveal('#pipeline-c',131);reveal('#routes',134);reveal('#direct',138);
  reveal('#storage',145);reveal('#limits',149);reveal('#today',153);reveal('#future',157);
  timeline.to({},{duration:1},163);
  const seek=document.querySelector('#seek');seek.max=timeline.duration();
  const play=document.querySelector('#play');
  function update(){
    const t=timeline.time();seek.value=t;
    document.querySelector('#clock').value=`${format(t)} / ${format(timeline.duration())}`;
    play.textContent=timeline.paused()||t===timeline.duration()?'▶ Play':'Ⅱ Pause';
    play.setAttribute('aria-label',timeline.paused()?'Play':'Pause');
    nav.querySelectorAll('button').forEach((b,i)=>{const active=t>=chapters[i][0]&&(t<chapters[i][1]||i===7);b.classList.toggle('active',active);if(active)b.setAttribute('aria-current','step');else b.removeAttribute('aria-current');});
  }
  function toggle(){if(timeline.time()>=timeline.duration())timeline.restart();else timeline.paused(!timeline.paused());update();}
  play.onclick=toggle;document.querySelector('#restart').onclick=()=>{timeline.pause(0);update();};
  seek.oninput=()=>{timeline.pause().seek(Number(seek.value));update();};
  document.querySelector('#speed').onchange=e=>timeline.timeScale(Number(e.target.value));
  nav.onclick=e=>{const b=e.target.closest('button');if(b){timeline.pause().seek(chapters[Number(b.dataset.chapter)][0]+.01);update();}};
  document.querySelector('#home').onclick=e=>{e.preventDefault();timeline.pause(0);update();};
  document.querySelector('#fullscreen').onclick=()=>{if(document.fullscreenElement)document.exitFullscreen();else document.querySelector('.player').requestFullscreen().catch(()=>{});};
  document.addEventListener('keydown',e=>{
    if(['INPUT','SELECT','BUTTON','TEXTAREA','A'].includes(document.activeElement.tagName))return;
    if(e.code==='Space'){e.preventDefault();toggle();}
    if(e.code==='ArrowLeft'||e.code==='ArrowRight'){e.preventDefault();timeline.pause().seek(Math.max(0,Math.min(timeline.duration(),timeline.time()+(e.code==='ArrowRight'?5:-5))));update();}
  });
  // Enables deterministic screenshots or a future frame-by-frame video export.
  window.planetLesson={timeline,query,bounds,weights,result,seek:t=>{timeline.pause().seek(t);update();}};
  timeline.seek(0);update();
}

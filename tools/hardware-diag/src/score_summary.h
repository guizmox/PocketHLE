/* Compact benchmark pages. Reports remain the complete, unmodified evidence. */
static char score_rows[3][12][20];
static U32 score_ready[3], score_failed[3], score_complete[3], score_case;
static const char *const integer_labels[]={"Legacy mixed","32-bit multiply/add","32-bit unsigned div/rem","32-bit signed div/rem","64-bit add/shift","64-bit multiply","64-bit unsigned divide","64-bit signed divide"};
static const char *const float_labels[]={"Legacy mixed","Float32 addition","Float32 multiplication","Float32 division","Float32 square root","Int32 / Float32","Float32 / Float64","Float64 add/multiply","Float64 division","Float64 square root"};
static const char *const gl_labels[]={"512 triangles","2048 triangles","8192 triangles","128 draw calls","Animated matrices","Rebuilt vertices","Solid fill","Texture nearest","Texture linear","Texture + blending","GLOBAL INDEX"};
static const char *const gl_ids[]={"geometry_512","geometry_2048","geometry_8192","draw_calls_128","matrix_animated","vertices_rebuilt","fill_solid","fill_texture_nearest","fill_texture_linear","fill_texture_blend"};
static U32 begins(const char*s,const char*p){while(*p)if(*s++!=*p++)return 0;return 1;}
static U32 equals(const char*s,const char*t){while(*s&&*s==*t){s++;t++;}return *s==*t;}
static void score_copy(char*out,const char*s){U32 i=0;while(s[i]&&i<19){out[i]=s[i];i++;}out[i]=0;}
static U32 score_value(const char*s){U32 n=0;while(*s>='0'&&*s<='9')n=n*10+(*s++-'0');return n;}
static void score_number(char*out,U32 value,U32 milli){char b[16];U32 n=decimal(b,milli?value/1000:value),i,j=0;
 for(i=0;i<n;i++){out[j++]=b[i];if(!milli&&i+1<n&&(n-i-1)%3==0)out[j++]=' ';}
 if(milli){out[j++]='.';out[j++]=(char)('0'+value/100%10);out[j++]=(char)('0'+value/10%10);out[j++]=(char)('0'+value%10);}out[j]=0;
}
static void score_reset(U32 suite){U32 i;if(suite<9||suite>11)return;score_case=0;score_ready[suite-9]=0;score_failed[suite-9]=0;score_complete[suite-9]=0;for(i=0;i<12;i++)score_copy(score_rows[suite-9][i],"--");}
static void score_feed(const char*s){U32 page,i;if(selected<9||selected>11)return;page=selected-9;
 if(equals(s,"BENCHMARK_RESULT PASS")||equals(s,"OGLESBENCH_RESULT PASS"))score_complete[page]=1;
 if(page<2){
  const char*legacy=page?"FINAL_FLOATING_SCORE_ITERATIONS_PER_SECOND=":"FINAL_INTEGER_SCORE_ITERATIONS_PER_SECOND=";
  if(begins(s,legacy))score_number(score_rows[page][0],score_value(s+length(legacy)),0);
  if(begins(s,"CASE_ID="))score_case=score_value(s+8)+1;
  if(score_case<12&&begins(s,"CASE_MEDIAN_ITERATIONS_PER_SECOND="))score_number(score_rows[page][score_case],score_value(s+length("CASE_MEDIAN_ITERATIONS_PER_SECOND=")),0);
  if(score_case<12&&begins(s,"CASE_RESULT FAIL"))score_copy(score_rows[page][score_case],"FAIL");
  if(begins(s,"FINAL_SCORE INVALID"))score_copy(score_rows[page][0],"FAIL");
 }else{
  if(begins(s,"TEST="))for(i=0;i<10;i++)if(equals(s+5,gl_ids[i])){score_case=i;break;}
  if(score_case<10&&begins(s,"MEDIAN_RENDER_MILLIFPS="))score_number(score_rows[page][score_case],score_value(s+length("MEDIAN_RENDER_MILLIFPS=")),1);
  if(score_case<10&&begins(s,"TEST_RESULT FAIL"))score_copy(score_rows[page][score_case],"FAIL");
  if(begins(s,"GLOBAL_INDEX_MILLI="))score_number(score_rows[page][10],score_value(s+length("GLOBAL_INDEX_MILLI=")),1);
 }
}
static U32 score_valid(U32 page){U32 i,n=page==0?8:page==1?10:11;if(!score_complete[page])return 0;for(i=0;i<n;i++)if(equals(score_rows[page][i],"--")||equals(score_rows[page][i],"FAIL"))return 0;return 1;}
static void score_draw(U32 dc){U32 page=selected-9,i,n=page==0?8:page==1?10:11;const char*const*labels=page==0?integer_labels:page==1?float_labels:gl_labels;
 SetTextColor(dc,score_failed[page]?0x004080ff:0x0080e0a0);text(dc,6,20,page==0?"INTEGER":page==1?"FLOATING POINT":"OPENGLES");text(dc,260,20,score_failed[page]?"FAIL":"PASS");
 SetTextColor(dc,0x00999999);text(dc,6,35,page<2?"Iterations/second - higher is faster":"Render FPS - higher is faster");
 for(i=0;i<n;i++){char*value=score_rows[page][i];SetTextColor(dc,equals(value,"FAIL")?0x004080ff:0x00eeeeee);text(dc,6,51+(int)i*13,labels[i]);text(dc,314-(int)length(value)*6,51+(int)i*13,value);}
 SetTextColor(dc,0x00999999);text(dc,6,201,page<2?"Legacy score remains comparable to v1.":"Global index is a score, not display FPS.");
 SetTextColor(dc,0x0099ccff);text(dc,6,219,"Left/Right: page  B/Tab: log  A: menu");
}
static void score_page(int direction){U32 n,page=selected-9;for(n=0;n<3;n++){page=(page+3+direction)%3;if(score_ready[page]){selected=page+9;dirty=1;break;}}}

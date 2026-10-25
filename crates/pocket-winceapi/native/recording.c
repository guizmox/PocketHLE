/* Opaque FFmpeg recording boundary; one encoder and muxer per worker. */
#include <stdint.h>
#include <errno.h>
#include <string.h>
#include <libavutil/opt.h>
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/error.h>
#include <libswscale/swscale.h>
typedef struct PocketRecording {
 AVFormatContext *format; AVCodecContext *video,*audio;
 AVStream *vs,*as; AVFrame *vf,*af; AVPacket *packet;
 struct SwsContext *scale; int input_width,input_height; int audio_count,header; int64_t audio_pts;
} PocketRecording;
int pocket_recording_available(void) {
 return avcodec_find_encoder_by_name("libopenh264") && avcodec_find_encoder(AV_CODEC_ID_AAC) && av_guess_format("mp4",NULL,NULL);
}
int pocket_recording_missing(void) {
 return (!avcodec_find_encoder_by_name("libopenh264") ? 1 : 0)
      | (!avcodec_find_encoder(AV_CODEC_ID_AAC) ? 2 : 0)
      | (!av_guess_format("mp4",NULL,NULL) ? 4 : 0);
}
void pocket_recording_error(int error,char *buffer,unsigned size){av_strerror(error,buffer,size);}
static void recording_free(PocketRecording *r) {
 if (!r) return;
 sws_freeContext(r->scale); av_frame_free(&r->vf); av_frame_free(&r->af);
 av_packet_free(&r->packet); avcodec_free_context(&r->video); avcodec_free_context(&r->audio);
 if(r->format){if(r->format->pb)avio_closep(&r->format->pb);avformat_free_context(r->format);} av_free(r);
}
static int recording_packets(PocketRecording *r,AVCodecContext *codec,AVStream *stream) {
 for (;;) {int rc=avcodec_receive_packet(codec,r->packet);if(rc==AVERROR(EAGAIN)||rc==AVERROR_EOF)return 0;if(rc<0)return rc;
  av_packet_rescale_ts(r->packet,codec->time_base,stream->time_base);r->packet->stream_index=stream->index;
  if(stream==r->vs && !r->packet->duration)r->packet->duration=av_rescale_q(1,codec->time_base,stream->time_base);
  rc=av_interleaved_write_frame(r->format,r->packet);av_packet_unref(r->packet);if(rc<0)return rc;
 }
}
static int recording_send(PocketRecording *r,AVCodecContext *codec,AVStream *stream,AVFrame *frame){
 int rc=avcodec_send_frame(codec,frame);if(rc<0)return rc;return recording_packets(r,codec,stream);
}
PocketRecording *pocket_recording_open(const char *path,int w,int h,int fps,int quality,int *error) {
 int rc=AVERROR(EINVAL);PocketRecording *r=NULL;
 if(w<=0||h<=0||w>8192||h>8192||fps<=0)goto fail;
 if(!pocket_recording_available()){rc=AVERROR_ENCODER_NOT_FOUND;goto fail;}
 r=av_mallocz(sizeof(*r));if(!r){rc=AVERROR(ENOMEM);goto fail;}
 rc=avformat_alloc_output_context2(&r->format,NULL,"mp4",path);if(rc<0)goto fail;
 const AVCodec *vc=avcodec_find_encoder_by_name("libopenh264"),*ac=avcodec_find_encoder(AV_CODEC_ID_AAC);
 r->video=avcodec_alloc_context3(vc);r->audio=avcodec_alloc_context3(ac);r->vs=avformat_new_stream(r->format,NULL);r->as=avformat_new_stream(r->format,NULL);
 r->vf=av_frame_alloc();r->af=av_frame_alloc();r->packet=av_packet_alloc();
 if(!r->video||!r->audio||!r->vs||!r->as||!r->vf||!r->af||!r->packet){rc=AVERROR(ENOMEM);goto fail;}
 r->input_width=w;r->input_height=h;
 r->video->width=(w+1)&~1;r->video->height=(h+1)&~1;r->video->pix_fmt=AV_PIX_FMT_YUV420P;r->video->time_base=(AVRational){1,fps};r->video->framerate=(AVRational){fps,1};
 r->video->bit_rate=(int64_t)w*h*fps/(quality==0?12:quality==2?3:6);int64_t floor=(int64_t)(quality==0?400000:quality==2?1600000:800000)*fps/30;if(r->video->bit_rate<floor)r->video->bit_rate=floor;
 r->video->rc_max_rate=r->video->bit_rate;r->video->rc_buffer_size=(int)r->video->bit_rate;r->video->rc_initial_buffer_occupancy=r->video->rc_buffer_size*3/4;
 r->video->gop_size=fps*2;r->video->max_b_frames=0;r->video->thread_count=2;
 r->audio->sample_fmt=AV_SAMPLE_FMT_FLTP;r->audio->sample_rate=48000;r->audio->time_base=(AVRational){1,48000};r->audio->bit_rate=quality==0?96000:quality==2?192000:128000;av_channel_layout_default(&r->audio->ch_layout,2);
 if(r->format->oformat->flags&AVFMT_GLOBALHEADER){r->video->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;r->audio->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;}
 rc=av_opt_set(r->video->priv_data,"profile","constrained_baseline",0);if(rc<0)goto fail;
 rc=av_opt_set(r->video->priv_data,"rc_mode","bitrate",0);if(rc<0)goto fail;
 rc=avcodec_open2(r->video,vc,NULL);if(rc<0)goto fail;rc=avcodec_open2(r->audio,ac,NULL);if(rc<0)goto fail;
 r->vs->time_base=r->video->time_base;r->as->time_base=r->audio->time_base;
 rc=avcodec_parameters_from_context(r->vs->codecpar,r->video);if(rc<0)goto fail;rc=avcodec_parameters_from_context(r->as->codecpar,r->audio);if(rc<0)goto fail;
 r->vf->format=r->video->pix_fmt;r->vf->width=r->video->width;r->vf->height=r->video->height;rc=av_frame_get_buffer(r->vf,32);if(rc<0)goto fail;
 r->af->format=r->audio->sample_fmt;r->af->sample_rate=48000;r->af->nb_samples=r->audio->frame_size;rc=av_channel_layout_copy(&r->af->ch_layout,&r->audio->ch_layout);if(rc<0)goto fail;
 rc=av_frame_get_buffer(r->af,0);if(rc<0)goto fail;
 r->scale=sws_getContext(w,h,AV_PIX_FMT_RGBA,w,h,AV_PIX_FMT_YUV420P,SWS_BILINEAR,NULL,NULL,NULL);if(!r->scale){rc=AVERROR(ENOMEM);goto fail;}
 rc=avio_open(&r->format->pb,path,AVIO_FLAG_WRITE);if(rc<0)goto fail;AVDictionary *mux_options=NULL;av_dict_set(&mux_options,"movflags","+faststart",0);
 rc=avformat_write_header(r->format,&mux_options);av_dict_free(&mux_options);if(rc<0)goto fail;r->header=1;*error=0;return r;
 fail:if(error)*error=rc;recording_free(r);return NULL;
}
int pocket_recording_video(PocketRecording *r,const uint8_t *rgba,int64_t pts) {
 int rc=av_frame_make_writable(r->vf);if(rc<0)return rc;const uint8_t *src[4]={rgba,NULL,NULL,NULL};int stride[4]={r->input_width*4,0,0,0};
 if(sws_scale(r->scale,src,stride,0,r->input_height,r->vf->data,r->vf->linesize)!=r->input_height)return AVERROR(EINVAL);
 /* H.264 4:2:0 requires even dimensions. Preserve every source pixel and
    extend only the final luma row/column, never resize or crop the picture. */
 if(r->video->width!=r->input_width)for(int y=0;y<r->input_height;y++) {
  uint8_t *row=r->vf->data[0]+y*r->vf->linesize[0];row[r->input_width]=row[r->input_width-1];
 }
 if(r->video->height!=r->input_height)memcpy(r->vf->data[0]+r->input_height*r->vf->linesize[0],r->vf->data[0]+(r->input_height-1)*r->vf->linesize[0],r->video->width);
 r->vf->pts=pts;return recording_send(r,r->video,r->vs,r->vf);
}
int pocket_recording_audio(PocketRecording *r,const int16_t *pcm,unsigned frames) {
 for(unsigned i=0;i<frames;i++) {
  if(r->audio_count==0){int rc=av_frame_make_writable(r->af);if(rc<0)return rc;}
  for(int c=0;c<2;c++)((float*)r->af->data[c])[r->audio_count]=pcm[i*2+c]/32768.0f;
  if(++r->audio_count==r->af->nb_samples){r->af->pts=r->audio_pts;r->audio_pts+=r->af->nb_samples;int rc=recording_send(r,r->audio,r->as,r->af);if(rc<0)return rc;r->audio_count=0;}
 }return 0;
}
int pocket_recording_close(PocketRecording *r) {
 if(!r)return 0;int rc=0,next;
 if(r->header){if(r->audio_count){for(int c=0;c<2;c++)for(int i=r->audio_count;i<r->af->nb_samples;i++)((float*)r->af->data[c])[i]=0;r->af->pts=r->audio_pts;rc=recording_send(r,r->audio,r->as,r->af);}
  next=recording_send(r,r->video,r->vs,NULL);if(rc>=0)rc=next;next=recording_send(r,r->audio,r->as,NULL);if(rc>=0)rc=next;next=av_write_trailer(r->format);if(rc>=0)rc=next;
 }recording_free(r);return rc;
}

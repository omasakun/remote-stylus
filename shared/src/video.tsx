import { forwardRef, useEffect, useImperativeHandle, useRef, type VideoHTMLAttributes } from 'react'

type PropsType = VideoHTMLAttributes<HTMLVideoElement> & {
  srcObject: MediaStream | null
}

export const Video = forwardRef<HTMLVideoElement, PropsType>(({ srcObject, ...props }, ref) => {
  const refVideo = useRef<HTMLVideoElement>(null)

  useImperativeHandle(ref, () => refVideo.current!, [refVideo])

  useEffect(() => {
    if (!refVideo.current) return
    refVideo.current.srcObject = srcObject
  }, [srcObject])

  return <video ref={refVideo} {...props} />
})

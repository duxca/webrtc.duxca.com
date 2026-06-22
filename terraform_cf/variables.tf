variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID for duxca.com."
  type        = string
}

variable "webrtc_dns_name" {
  description = "DNS record name for the WebRTC Cloud Run domain mapping."
  type        = string
  default     = "webrtc.duxca.com"
}

variable "webrtc_dns_content" {
  description = "DNS CNAME target for the WebRTC Cloud Run domain mapping."
  type        = string
  default     = "ghs.googlehosted.com"
}


provider "cloudflare" {}

resource "cloudflare_dns_record" "webrtc" {
  zone_id = var.cloudflare_zone_id
  name    = var.webrtc_dns_name
  content = var.webrtc_dns_content
  type    = "CNAME"
  proxied = false
  ttl     = 60
}


#!/bin/bash
#
# Script de déploiement Chemins Noirs
# Cross-compilation locale + déploiement sur VPS
#

set -e

# ============================================================================
# CONFIGURATION - À MODIFIER
# ============================================================================

# Connexion SSH au VPS
VPS_USER="root"                              # Utilisateur SSH
VPS_HOST="VPS-779132.ssh.vps1euro.fr"        # Proxy SSH IPv4
VPS_PORT="9999"                              # Port du proxy
VPS_APP_DIR="/opt/cheminsnoirs"              # Répertoire d'installation sur le VPS

# Domaine (optionnel, pour la config nginx)
DOMAIN="cheminsnoirs.mooo.com"               # Domaine pour nginx

# Répertoire du projet (détection automatique)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"

# Fichier PBF pour le routage (à placer sur le VPS dans /opt/cheminsnoirs/data/)
PBF_FILENAME="rhone-alpes-251111.osm.pbf"

# ============================================================================
# COULEURS
# ============================================================================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ============================================================================
# VÉRIFICATIONS
# ============================================================================

check_config() {
    if [ -z "$VPS_HOST" ]; then
        log_error "Configurez VPS_HOST dans le script (ligne 15)"
    fi
}

check_dependencies() {
    log_info "Vérification des dépendances locales..."

    command -v ssh >/dev/null 2>&1 || log_error "ssh non installé"
    command -v scp >/dev/null 2>&1 || log_error "scp non installé"

    log_success "Dépendances OK"
}

# ============================================================================
# BUILD LOCAL
# ============================================================================

build_backend() {
    log_info "Compilation du backend Rust (release)..."

    cd "$PROJECT_DIR"

    if [ -f "build-docker.sh" ]; then
        log_info "Utilisation du build Docker pour compatibilité VPS..."
        bash build-docker.sh
    else
        cargo build --release -p backend --bin backend_partial
    fi

    if [ ! -f "target/release/backend_partial" ]; then
        log_error "Échec de la compilation"
    fi

    BINARY_SIZE=$(du -h target/release/backend_partial | cut -f1)
    log_success "Backend compilé ($BINARY_SIZE)"
}

build_frontend() {
    log_info "Compilation du frontend Elm..."

    cd "$PROJECT_DIR/frontend-elm"

    if [ ! -d "node_modules" ]; then
        npm install
    fi

    npm run build

    log_success "Frontend compilé"
}

# ============================================================================
# PACKAGING
# ============================================================================

create_package() {
    log_info "Création du package de déploiement..."

    cd "$PROJECT_DIR"

    PACKAGE_DIR="deploy_package"
    rm -rf "$PACKAGE_DIR"
    mkdir -p "$PACKAGE_DIR"

    # Copier le binaire
    cp target/release/backend_partial "$PACKAGE_DIR/"

    # Copier le frontend (fichiers statiques Vite build)
    mkdir -p "$PACKAGE_DIR/frontend"
    cp -r frontend-elm/dist/* "$PACKAGE_DIR/frontend/"

    # Créer les répertoires nécessaires
    mkdir -p "$PACKAGE_DIR/data/cache"

    # ── Service systemd avec sandbox complet ──
    cat > "$PACKAGE_DIR/cheminsnoirs.service" << 'EOF'
[Unit]
Description=Chemins Noirs - Générateur GPX anti-bitume
After=network.target postgresql.service
Wants=postgresql.service

[Service]
Type=simple
User=cheminsnoirs
Group=cheminsnoirs
WorkingDirectory=/opt/cheminsnoirs
ExecStart=/opt/cheminsnoirs/backend_partial
Restart=always
RestartSec=5

# Environnement
Environment=RUST_LOG=info
Environment=PBF_PATH=/opt/cheminsnoirs/data/rhone-alpes-251111.osm.pbf
Environment=CACHE_DIR=/opt/cheminsnoirs/data/cache

# ── Sécurité : sandbox systemd complet ──
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/cheminsnoirs/data
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
ProtectClock=true
ProtectHostname=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallFilter=~@privileged @resources
CapabilityBoundingSet=
AmbientCapabilities=

# Limites ressources (protection DoS)
LimitNOFILE=65536
LimitNPROC=512

[Install]
WantedBy=multi-user.target
EOF

    # ── Config nginx avec sécurité renforcée ──
    cat > "$PACKAGE_DIR/nginx-cheminsnoirs.conf" << 'NGINX_EOF'
# Rate limiting zones (avant le bloc server)
limit_req_zone $binary_remote_addr zone=api_limit:10m rate=10r/s;
limit_req_zone $binary_remote_addr zone=static_limit:10m rate=30r/s;
limit_conn_zone $binary_remote_addr zone=conn_limit:10m;

server {
    listen 80;
    listen [::]:80;
    server_name cheminsnoirs.mooo.com;

    # ── Security Headers ──
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "DENY" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Permissions-Policy "camera=(), microphone=(), geolocation=(self), payment=()" always;
    add_header Content-Security-Policy "default-src 'self'; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://unpkg.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data: blob: https://tile.openstreetmap.org https://*.tile.openstreetmap.org https://*.basemaps.cartocdn.com https://*.arcgisonline.com https://server.arcgisonline.com https://s3.amazonaws.com; connect-src 'self' https://tile.openstreetmap.org https://*.tile.openstreetmap.org https://*.basemaps.cartocdn.com https://api.maptiler.com https://demotiles.maplibre.org https://nominatim.openstreetmap.org https://*.arcgisonline.com https://s3.amazonaws.com; worker-src 'self' blob:;" always;

    # Masquer la version nginx
    server_tokens off;

    # Limiter la taille des requêtes (protection upload malveillant)
    client_max_body_size 1m;
    client_body_timeout 10s;
    client_header_timeout 10s;

    # Limite de connexions simultanées par IP
    limit_conn conn_limit 20;

    # Frontend Elm (fichiers statiques Vite)
    root /opt/cheminsnoirs/frontend;
    index index.html;

    # Gzip compression
    gzip on;
    gzip_vary on;
    gzip_proxied any;
    gzip_comp_level 6;
    gzip_types text/plain text/css application/json application/javascript text/xml application/xml application/xml+rss text/javascript;

    # Static files avec cache long (Vite hash les noms)
    location /assets/ {
        expires 1y;
        add_header Cache-Control "public, immutable";
        limit_req zone=static_limit burst=50 nodelay;
    }

    # Fichiers statiques racine (style.css, manifest.json, etc.)
    location ~* \.(css|js|json|ico|png|jpg|jpeg|gif|svg|woff2?)$ {
        expires 7d;
        add_header Cache-Control "public";
        limit_req zone=static_limit burst=50 nodelay;
    }

    # SPA fallback
    location / {
        try_files $uri $uri/ /index.html;
        limit_req zone=static_limit burst=20 nodelay;
    }

    # API proxy → backend Rust sur port 8090
    location /api/ {
        # Rate limiting API (10 req/s par IP, burst de 20)
        limit_req zone=api_limit burst=20 nodelay;
        limit_req_status 429;

        proxy_pass http://127.0.0.1:8090/api/;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Pas de buffering pour les réponses streamed
        proxy_buffering on;
        proxy_buffer_size 8k;
        proxy_buffers 8 8k;

        # Timeout pour le routage (première requête peut être lente)
        proxy_read_timeout 300s;
        proxy_send_timeout 30s;
        proxy_connect_timeout 5s;
    }

    # Bloquer l'accès aux fichiers cachés
    location ~ /\. {
        deny all;
        return 404;
    }

    # Bloquer les extensions dangereuses
    location ~* \.(php|asp|aspx|jsp|cgi|pl|py|sh|bash|env|git|svn|htaccess|htpasswd|ini|log|sql|bak|swp|tmp)$ {
        deny all;
        return 404;
    }
}
NGINX_EOF

    # ── Script d'installation avec hardening VPS ──
    cat > "$PACKAGE_DIR/install.sh" << 'INSTALL_EOF'
#!/bin/bash
set -e

echo "=== Installation de Chemins Noirs (avec sécurité renforcée) ==="

# ── Créer l'utilisateur système (sans shell, sans home interactif) ──
if ! id "cheminsnoirs" &>/dev/null; then
    useradd -r -s /usr/sbin/nologin -d /opt/cheminsnoirs -M cheminsnoirs
    echo "Utilisateur cheminsnoirs créé (nologin)"
fi

# ── Créer les répertoires ──
mkdir -p /opt/cheminsnoirs/{data/cache,frontend}

# ── Copier les fichiers ──
cp backend_partial /opt/cheminsnoirs/
chmod 750 /opt/cheminsnoirs/backend_partial
cp -r frontend/* /opt/cheminsnoirs/frontend/
cp -r data/* /opt/cheminsnoirs/data/ 2>/dev/null || true

# ── Permissions strictes ──
chown -R cheminsnoirs:cheminsnoirs /opt/cheminsnoirs
chmod 755 /opt/cheminsnoirs
chmod 755 /opt/cheminsnoirs/frontend
find /opt/cheminsnoirs/frontend -type f -exec chmod 644 {} \;
find /opt/cheminsnoirs/frontend -type d -exec chmod 755 {} \;
chmod 750 /opt/cheminsnoirs/data
chmod 750 /opt/cheminsnoirs/data/cache

# ── Vérifier que le PBF est présent ──
if [ ! -f /opt/cheminsnoirs/data/rhone-alpes-251111.osm.pbf ]; then
    echo ""
    echo "IMPORTANT: Le fichier PBF n'est pas encore présent."
    echo "   Transférez-le manuellement sur le VPS :"
    echo "   scp rhone-alpes-251111.osm.pbf root@VPS:/opt/cheminsnoirs/data/"
    echo ""
fi

# ── Installer le service systemd ──
cp cheminsnoirs.service /etc/systemd/system/
chmod 644 /etc/systemd/system/cheminsnoirs.service
systemctl daemon-reload
systemctl enable cheminsnoirs

# ── Installer nginx config ──
cp nginx-cheminsnoirs.conf /etc/nginx/sites-available/cheminsnoirs
ln -sf /etc/nginx/sites-available/cheminsnoirs /etc/nginx/sites-enabled/
# Ne PAS supprimer default (Take It Easy cohabite sur le même VPS)

# ── Hardening nginx global (si pas déjà fait) ──
if ! grep -q "server_tokens off" /etc/nginx/nginx.conf; then
    sed -i '/http {/a\    server_tokens off;' /etc/nginx/nginx.conf
fi

# Tester et recharger nginx
nginx -t && systemctl reload nginx

# ── Firewall (ufw) ──
if command -v ufw >/dev/null 2>&1; then
    echo "Configuration du firewall (ufw)..."
    ufw default deny incoming 2>/dev/null || true
    ufw default allow outgoing 2>/dev/null || true
    ufw allow ssh 2>/dev/null || true
    ufw allow 80/tcp 2>/dev/null || true
    ufw allow 443/tcp 2>/dev/null || true
    # Ne PAS exposer le port 8090 (backend interne uniquement)
    ufw --force enable 2>/dev/null || true
    echo "Firewall configuré (SSH + HTTP + HTTPS uniquement)"
fi

# ── Fail2ban (protection brute-force) ──
if command -v fail2ban-client >/dev/null 2>&1; then
    echo "Fail2ban détecté, configuration nginx jail..."
    cat > /etc/fail2ban/jail.d/cheminsnoirs.conf << 'F2B_EOF'
[nginx-limit-req]
enabled = true
port = http,https
logpath = /var/log/nginx/error.log
maxretry = 10
findtime = 60
bantime = 600

[nginx-botsearch]
enabled = true
port = http,https
logpath = /var/log/nginx/access.log
maxretry = 5
findtime = 60
bantime = 3600
F2B_EOF
    systemctl restart fail2ban 2>/dev/null || true
fi

# ── Démarrer le service (seulement si PBF présent) ──
if [ -f /opt/cheminsnoirs/data/rhone-alpes-251111.osm.pbf ]; then
    systemctl restart cheminsnoirs
    echo ""
    echo "=== Installation terminée (service démarré) ==="
    echo "Service: systemctl status cheminsnoirs"
    echo "Logs:    journalctl -u cheminsnoirs -f"
else
    echo ""
    echo "=== Installation terminée (service non démarré — PBF manquant) ==="
    echo "Après avoir copié le PBF: systemctl start cheminsnoirs"
fi

echo ""
echo "Sécurité:"
echo "  - Sandbox systemd complet (NoNewPrivileges, ProtectSystem, MemoryDenyWriteExecute...)"
echo "  - nginx: rate limiting, CSP, headers sécurité, version cachée"
echo "  - Firewall: seuls SSH/HTTP/HTTPS ouverts (port 8090 interne)"
echo "  - Backend tourne en utilisateur dédié sans shell"
echo ""
echo "Prochaines étapes recommandées:"
echo "  1. Configurer HTTPS: apt install certbot python3-certbot-nginx && certbot --nginx"
echo "  2. Installer fail2ban: apt install fail2ban"
echo "  3. Vérifier les mises à jour: apt update && apt upgrade"
echo ""
INSTALL_EOF
    chmod +x "$PACKAGE_DIR/install.sh"

    PACKAGE_SIZE=$(du -sh "$PACKAGE_DIR" | cut -f1)
    log_success "Package créé ($PACKAGE_SIZE)"
}

# ============================================================================
# DÉPLOIEMENT
# ============================================================================

deploy_to_vps() {
    log_info "Déploiement sur le VPS ($VPS_USER@$VPS_HOST:$VPS_PORT)..."

    cd "$PROJECT_DIR"

    # Test de connexion SSH
    log_info "Test de connexion SSH..."
    ssh -p "$VPS_PORT" -o ConnectTimeout=10 "$VPS_USER@$VPS_HOST" "echo 'Connexion OK'" || log_error "Impossible de se connecter au VPS"

    # Créer le répertoire temporaire sur le VPS
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "mkdir -p /tmp/cheminsnoirs_deploy"

    # Transfert du package
    log_info "Transfert des fichiers..."
    scp -P "$VPS_PORT" -r deploy_package/* "$VPS_USER@$VPS_HOST:/tmp/cheminsnoirs_deploy/"

    # Exécution du script d'installation
    log_info "Installation sur le VPS..."
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "cd /tmp/cheminsnoirs_deploy && bash install.sh"

    # Nettoyage
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "rm -rf /tmp/cheminsnoirs_deploy"

    log_success "Déploiement terminé!"
}

setup_vps_prerequisites() {
    log_info "Installation des prérequis sur le VPS..."

    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" << 'REMOTE_EOF'
set -e
apt update
apt install -y nginx ufw fail2ban

# PostgreSQL (optionnel, pour sauvegarder les tracés)
apt install -y postgresql postgresql-contrib || true

# Activer les mises à jour de sécurité automatiques
apt install -y unattended-upgrades
dpkg-reconfigure -plow unattended-upgrades 2>/dev/null || true

# Créer le répertoire temporaire
mkdir -p /tmp/cheminsnoirs_deploy

echo "Prérequis installés (nginx, ufw, fail2ban, unattended-upgrades)"
REMOTE_EOF

    log_success "Prérequis VPS OK"
}

upload_pbf() {
    check_config

    PBF_LOCAL="$PROJECT_DIR/backend/data/$PBF_FILENAME"

    if [ ! -f "$PBF_LOCAL" ]; then
        log_error "Fichier PBF non trouvé: $PBF_LOCAL"
    fi

    PBF_SIZE=$(du -h "$PBF_LOCAL" | cut -f1)
    log_info "Transfert du PBF ($PBF_SIZE) — cela peut prendre plusieurs minutes..."

    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "mkdir -p /opt/cheminsnoirs/data"
    scp -P "$VPS_PORT" "$PBF_LOCAL" "$VPS_USER@$VPS_HOST:/opt/cheminsnoirs/data/$PBF_FILENAME"
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "chown cheminsnoirs:cheminsnoirs /opt/cheminsnoirs/data/$PBF_FILENAME && chmod 640 /opt/cheminsnoirs/data/$PBF_FILENAME"

    log_success "PBF transféré!"
}

# ============================================================================
# COMMANDES
# ============================================================================

show_help() {
    echo "Usage: $0 <commande>"
    echo ""
    echo "Commandes disponibles:"
    echo "  build       Compiler le backend et frontend localement"
    echo "  package     Créer le package de déploiement"
    echo "  deploy      Déployer sur le VPS (build + package + upload)"
    echo "  setup-vps   Installer les prérequis sur le VPS (nginx, ufw, fail2ban)"
    echo "  upload-pbf  Transférer le fichier PBF sur le VPS (~500 MB)"
    echo "  full        Tout faire: setup-vps + build + package + deploy"
    echo "  status      Vérifier le statut du service sur le VPS"
    echo "  logs        Afficher les logs du service sur le VPS"
    echo "  restart     Redémarrer le service sur le VPS"
    echo ""
    echo "Premier déploiement:"
    echo "  1. Modifier VPS_HOST/VPS_PORT/VPS_USER dans ce script"
    echo "  2. $0 full"
    echo "  3. $0 upload-pbf    (transfert du fichier PBF ~500 MB)"
    echo "  4. $0 restart       (démarrer le service)"
    echo "  5. certbot --nginx  (HTTPS)"
    echo ""
}

cmd_build() {
    check_dependencies
    build_backend
    build_frontend
    log_success "Build complet terminé!"
}

cmd_package() {
    create_package
}

cmd_deploy() {
    check_config
    check_dependencies
    build_backend
    build_frontend
    create_package
    deploy_to_vps
}

cmd_full() {
    check_config
    check_dependencies
    setup_vps_prerequisites
    build_backend
    build_frontend
    create_package
    deploy_to_vps

    echo ""
    log_success "=== Déploiement complet terminé! ==="
    echo ""
    echo "N'oubliez pas de transférer le fichier PBF:"
    echo "  $0 upload-pbf"
    echo ""
    echo "Puis démarrez le service:"
    echo "  $0 restart"
    echo ""
    echo "Puis configurez HTTPS:"
    echo "  ssh $VPS_USER@$VPS_HOST -p $VPS_PORT"
    echo "  certbot --nginx -d votredomaine.com"
    echo ""
}

cmd_status() {
    check_config
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "systemctl status cheminsnoirs"
}

cmd_logs() {
    check_config
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "journalctl -u cheminsnoirs -f"
}

cmd_restart() {
    check_config
    ssh -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" "systemctl restart cheminsnoirs"
    log_success "Service redémarré"
}

# ============================================================================
# MAIN
# ============================================================================

case "${1:-help}" in
    build)      cmd_build ;;
    package)    cmd_package ;;
    deploy)     cmd_deploy ;;
    setup-vps)  check_config; setup_vps_prerequisites ;;
    upload-pbf) upload_pbf ;;
    full)       cmd_full ;;
    status)     cmd_status ;;
    logs)       cmd_logs ;;
    restart)    cmd_restart ;;
    help|--help|-h) show_help ;;
    *)          log_error "Commande inconnue: $1. Utilisez '$0 help'" ;;
esac

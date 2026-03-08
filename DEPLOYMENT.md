# Production Deployment Guide

Déploiement de Chemins Noirs sur un VPS via GitHub.

## Architecture

```
┌─────────────────┐     HTTPS         ┌─────────────────┐
│   Browser       │◄────────────────►│   nginx :443     │
└─────────────────┘                  │  + rate limiting │
                                     │  + CSP headers   │
                                     └────────┬─────────┘
                                              │
                              ┌───────────────┴───────────────┐
                              │                               │
                              ▼                               ▼
                     ┌───────────────┐              ┌───────────────┐
                     │ Static Files  │              │ API Proxy     │
                     │ /             │              │ /api/*        │
                     │ (Elm + Vite)  │              │ → :8080       │
                     └───────────────┘              └───────┬───────┘
                                                           │ (localhost only)
                                                           ▼
                                                  ┌───────────────┐
                                                  │ Rust Backend  │
                                                  │ (axum :8080)  │
                                                  │ sandboxed     │
                                                  │ systemd       │
                                                  └───────────────┘
```

## CI/CD : Déploiement automatique via GitHub Actions

Chaque push sur `master` déclenche automatiquement :
1. Build du backend Rust (release, Ubuntu 22.04)
2. Build du frontend Elm + Vite
3. Déploiement sur le VPS via SSH

### Setup initial (une seule fois)

**1. Ajouter la clé SSH dans GitHub Secrets :**

```bash
# Générer une clé SSH dédiée au déploiement
ssh-keygen -t ed25519 -C "github-deploy" -f ~/.ssh/id_deploy_cheminsnoirs -N ""

# Copier la clé publique sur le VPS
ssh-copy-id -p 9999 -i ~/.ssh/id_deploy_cheminsnoirs.pub root@VPS-779132.ssh.vps1euro.fr

# Copier la clé privée dans le presse-papier
cat ~/.ssh/id_deploy_cheminsnoirs
```

Puis dans GitHub : **Settings → Secrets → Actions → New secret**
- Nom : `VPS_SSH_KEY`
- Valeur : le contenu de la clé privée

**2. Lancer le setup VPS (workflow manuel) :**

Dans GitHub : **Actions → "Setup VPS (first time)" → Run workflow**

Cela installe nginx, firewall, fail2ban, systemd service sur le VPS.

**3. Transférer le fichier PBF :**

```bash
./deploy.sh upload-pbf
```

**4. HTTPS (sur le VPS) :**

```bash
ssh -p 9999 root@VPS-779132.ssh.vps1euro.fr
apt install certbot python3-certbot-nginx
certbot --nginx -d votredomaine.example.com
```

### Ensuite : push → deploy automatique

```bash
git push origin master
# → GitHub Actions build + déploie automatiquement
```

## Déploiement manuel (alternative)

Si vous préférez déployer depuis votre machine locale :

```bash
# Modifier la config SSH dans deploy.sh (lignes 14-16)
./deploy.sh full         # Premier déploiement complet
./deploy.sh upload-pbf   # Transférer le PBF
./deploy.sh restart      # Démarrer
```

## PostgreSQL (optionnel)

```bash
ssh user@votre-vps.example.com

sudo -u postgres createuser cheminsnoirs
sudo -u postgres createdb cheminsnoirs -O cheminsnoirs

sudo systemctl edit cheminsnoirs --force
# Ajouter :
# [Service]
# Environment=DATABASE_URL=postgresql://cheminsnoirs@localhost/cheminsnoirs

sudo systemctl daemon-reload
sudo systemctl restart cheminsnoirs
```

## Sécurité

Le déploiement inclut les protections suivantes :

### Systemd (sandbox complet)

| Protection | Effet |
|-----------|-------|
| `NoNewPrivileges` | Empêche l'escalade de privilèges |
| `ProtectSystem=strict` | Système de fichiers en lecture seule (sauf data/) |
| `ProtectHome=true` | Pas d'accès aux répertoires home |
| `PrivateTmp=true` | /tmp isolé par processus |
| `PrivateDevices=true` | Pas d'accès aux périphériques |
| `ProtectKernel*` | Pas d'accès aux tunables/modules/logs kernel |
| `RestrictAddressFamilies` | IPv4, IPv6 et Unix sockets uniquement |
| `MemoryDenyWriteExecute` | Empêche l'injection de code en mémoire |
| `SystemCallFilter` | Limite les appels système autorisés |
| `CapabilityBoundingSet=` | Aucune capability Linux accordée |
| `LimitNOFILE/LimitNPROC` | Limites de ressources anti-DoS |

### Nginx

| Protection | Effet |
|-----------|-------|
| Rate limiting API | 10 req/s par IP (burst 20) |
| Rate limiting statique | 30 req/s par IP |
| Limite connexions | 20 simultanées par IP |
| `client_max_body_size 1m` | Rejette les gros uploads |
| `server_tokens off` | Cache la version nginx |
| CSP header | Politique stricte (self + CDN tiles) |
| `X-Frame-Options: DENY` | Anti-clickjacking |
| `X-Content-Type-Options` | Anti-MIME sniffing |
| `Permissions-Policy` | Restreint les APIs navigateur |
| Blocage fichiers cachés | `.env`, `.git`, etc. inaccessibles |
| Blocage extensions | `.php`, `.sql`, `.env`, `.log`, etc. |

### Réseau (ufw firewall)

- Seuls les ports SSH, 80, 443 sont ouverts
- Le port 8080 (backend) est **interne uniquement** (localhost)
- fail2ban protège contre le brute-force SSH et les abus nginx

### VPS

- `unattended-upgrades` : mises à jour de sécurité automatiques
- Utilisateur dédié `cheminsnoirs` sans shell (`/usr/sbin/nologin`)
- Permissions strictes sur les fichiers (750 pour le binaire, 644 pour le frontend)

## Commandes de déploiement

| Commande | Description |
|----------|-------------|
| `./build-docker.sh` | Build Docker (glibc 2.35 compat) |
| `./deploy.sh build` | Compiler backend + frontend |
| `./deploy.sh package` | Créer le package de déploiement |
| `./deploy.sh deploy` | Full deploy (build + package + upload) |
| `./deploy.sh upload-pbf` | Transférer le fichier PBF (~500 MB) |
| `./deploy.sh full` | Tout : setup-vps + build + deploy |
| `./deploy.sh status` | Vérifier le statut du service |
| `./deploy.sh logs` | Voir les logs du service |
| `./deploy.sh restart` | Redémarrer le service |

## Structure sur le VPS

```
/opt/cheminsnoirs/
├── backend_partial           # Binaire Rust (750, cheminsnoirs:cheminsnoirs)
├── frontend/                 # Elm SPA (644/755, fichiers statiques Vite)
│   ├── index.html
│   ├── style.css
│   └── assets/
└── data/                     # (750, écriture limitée)
    ├── rhone-alpes-251111.osm.pbf  # Données routage (~500 MB)
    └── cache/                       # Cache graphe postcard
```

## Mise à jour

```bash
# Redéployer après des changements de code
./deploy.sh deploy

# Le PBF n'a pas besoin d'être re-transféré
```

## Troubleshooting

| Problème | Solution |
|----------|----------|
| `GLIBC_2.xx not found` | Rebuilder avec Docker : `./build-docker.sh` |
| `502 Bad Gateway` | `./deploy.sh status` puis `./deploy.sh logs` |
| `429 Too Many Requests` | Rate limiting nginx actif (10 req/s API) |
| Routage lent (1er appel) | Normal : PBF parsé à la demande (~11s), puis cache |
| `PBF not found` | `./deploy.sh upload-pbf` |

## Commandes utiles sur le VPS

```bash
systemctl status cheminsnoirs
journalctl -u cheminsnoirs -f
systemctl restart cheminsnoirs
nginx -t && systemctl reload nginx
ufw status                               # Firewall
fail2ban-client status                   # Bans actifs
ss -tlnp | grep -E ':(80|443|8080)'     # Ports
certbot renew --dry-run                  # Test renouvellement HTTPS
```

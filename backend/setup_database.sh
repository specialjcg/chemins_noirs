#!/bin/bash
# PostgreSQL Database Setup Script for Chemins Noirs

set -e

echo "🗄️  Configuration de la base de données PostgreSQL pour Chemins Noirs"
echo ""

# Lire le mot de passe
read -sp "Entrez un mot de passe pour l'utilisateur 'chemins_user': " DB_PASSWORD
echo ""

# Se connecter à PostgreSQL en tant que superutilisateur
echo "🔧 Création de la base de données et de l'utilisateur..."
sudo -u postgres psql <<EOF
-- Créer la base de données
CREATE DATABASE chemins_noirs;

-- Créer l'utilisateur
CREATE USER chemins_user WITH PASSWORD '$DB_PASSWORD';

-- Donner tous les privilèges
GRANT ALL PRIVILEGES ON DATABASE chemins_noirs TO chemins_user;

-- Se connecter à la base chemins_noirs pour donner les permissions sur le schéma
\c chemins_noirs

-- Donner les permissions sur le schéma public
GRANT ALL ON SCHEMA public TO chemins_user;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO chemins_user;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO chemins_user;

-- Permissions par défaut pour les futures tables
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO chemins_user;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON SEQUENCES TO chemins_user;

\q
EOF

echo "✅ Base de données créée avec succès!"
echo ""

# Mettre à jour le fichier .env
echo "📝 Mise à jour du fichier .env..."
cd "$(dirname "$0")"

# Échapper les caractères spéciaux dans le mot de passe pour l'URL
ESCAPED_PASSWORD=$(echo "$DB_PASSWORD" | sed 's/[@:/]/%&/g')

# Mettre à jour DATABASE_URL dans .env
if [ -f .env ]; then
    sed -i "s|DATABASE_URL=.*|DATABASE_URL=postgresql://chemins_user:${ESCAPED_PASSWORD}@localhost/chemins_noirs|" .env
    echo "✅ Fichier .env mis à jour"
else
    echo "⚠️  Fichier .env non trouvé"
fi

echo ""
echo "🎉 Configuration terminée!"
echo ""
echo "Pour tester la connexion:"
echo "  psql -U chemins_user -d chemins_noirs -h localhost"
echo ""
echo "Pour démarrer le backend:"
echo "  cd backend"
echo "  cargo run --bin backend_partial"

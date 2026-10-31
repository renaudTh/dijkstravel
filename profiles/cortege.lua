-- Profil OSRM « cortège » : les rues de la voiture (/opt/car.lua) et les rues piétonnes, dans les deux sens,
-- sans interdictions de tourner ni restrictions d'accès des véhicules (zones piétonnes, bornes) : un cortège
-- défile dans les rues, pas sur les chemins ni les escaliers.
-- Préparation : osrm-extract -p /profiles/cortege.lua /data/villes-cortege.osm.pbf (voir docker-compose.yml).

package.path = '/opt/?.lua;' .. package.path
local car = dofile('/opt/car.lua')
local car_setup = car.setup

car.setup = function()
  local profile = car_setup()
  profile.oneway_handling = false
  profile.properties.use_turn_restrictions = false
  -- Le plus court, pas le plus rapide : un cortège ne cherche pas les grands axes.
  profile.properties.weight_name = 'distance'
  profile.access_tags_hierarchy = Sequence { 'foot', 'access' }
  profile.speeds.highway.pedestrian = 10
  profile.barrier_whitelist['bollard'] = true
  -- Un point de passage au milieu d'une rue ne force pas à continuer tout droit : pas de boucle pour revenir.
  profile.properties.continue_straight_at_waypoint = false
  return profile
end

return car

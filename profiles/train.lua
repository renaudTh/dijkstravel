-- Profil OSRM « train » : uniquement les voies ferrées (railway=rail, narrow_gauge).
-- Préparation : osrm-extract -p /profiles/train.lua /data/france-rail.osm.pbf (voir docker-compose.yml).

api_version = 4

-- Durée affichée d'une manœuvre impossible ; son poids, lui, est le maximum d'OSRM (interdit).
-- (Une durée de plusieurs heures fait planter osrm-extract sur un dépassement numérique.)
SHARP_TURN_PENALTY = 3000

function setup()
  return {
    properties = {
      -- Poids distinct de la durée : il permet d'interdire une manœuvre sans fausser les temps.
      weight_name                   = 'routability',
      max_speed_for_map_matching    = 220/3.6,
      call_tagless_node_function    = false,
      -- Rebrousser chemin dans une gare (terminus, gare en impasse) est normal…
      continue_straight_at_waypoint = false,
      -- …mais jamais en pleine voie.
      u_turn_penalty                = SHARP_TURN_PENALTY,
      use_turn_restrictions         = false,
    },
    default_mode  = mode.train,
    default_speed = 100,
  }
end

function process_node(profile, node, result, relations)
end

function process_way(profile, way, result, relations)
  local railway = way:get_value_by_key('railway')
  if railway ~= 'rail' and railway ~= 'narrow_gauge' then
    return
  end
  local usage = way:get_value_by_key('usage')
  if usage == 'military' or usage == 'test' then
    return
  end

  local speed = profile.default_speed
  local maxspeed = tonumber(way:get_value_by_key('maxspeed') or '')
  if maxspeed and maxspeed > 0 then
    -- Vitesse commerciale moyenne nettement sous la vitesse limite.
    speed = math.min(maxspeed, 320) * 0.8
  end
  -- Voies de garage et de triage : praticables pour atteindre un quai, mais pas comme raccourci.
  local service = way:get_value_by_key('service')
  if service == 'siding' or service == 'yard' or service == 'spur' or service == 'crossover' then
    speed = 15
  end

  result.forward_mode = mode.train
  result.backward_mode = mode.train
  result.forward_speed = speed
  result.backward_speed = speed
  -- Poids « routability » : sans taux explicite, OSRM écarte la voie. Taux = vitesse en m/s,
  -- donc poids d'un tronçon = sa durée.
  result.forward_rate = speed / 3.6
  result.backward_rate = speed / 3.6
  result.name = way:get_value_by_key('name') or ''
end

function process_turn(profile, turn)
  -- `turn.angle` : déviation par rapport à la ligne droite, en degrés (0 = tout droit).
  -- Un train ne prend pas d'aiguillage à angle vif : au-delà de 45°, quasi interdit.
  turn.weight = turn.duration
  if turn.is_u_turn or math.abs(turn.angle) > 45 then
    turn.duration = SHARP_TURN_PENALTY
    turn.weight = constants.max_turn_weight
  end
end

return {
  setup = setup,
  process_way = process_way,
  process_node = process_node,
  process_turn = process_turn
}

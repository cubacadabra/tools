"""Bakeable fabric/color/contact detail, using only the supported color map.

AO is baked without a directional key light; the live renderer still lights
the actual surface. No beauty-render lighting or highlights are painted in.
"""
import bpy


def material(name, color, kind='plain', tint=False, display=None):
    mat=bpy.data.materials.new(name)
    mat.use_nodes=True
    mat['cubaUseAvatarTint']=tint
    mat['displayColor']=display or color
    mat['roughness']=.52 if kind=='hair' else .86
    n=mat.node_tree.nodes; links=mat.node_tree.links
    n.clear()
    out=n.new('ShaderNodeOutputMaterial')
    emit=n.new('ShaderNodeEmission')
    links.new(emit.outputs[0],out.inputs['Surface'])
    tex=n.new('ShaderNodeTexCoord')
    # Broad yarn/dye variation must survive the runtime's 256px atlas. Fine
    # fibers are a quiet second scale, not the source of the garment's form.
    noise=n.new('ShaderNodeTexNoise')
    noise.inputs['Scale'].default_value={'fleece':28,'denim':24,'rib':32,'hair':18}.get(kind,100)
    noise.inputs['Detail'].default_value=2; noise.inputs['Roughness'].default_value=.72
    links.new(tex.outputs['Object'],noise.inputs['Vector'])
    ramp=n.new('ShaderNodeValToRGB')
    strength={'plain':.025,'fleece':.25,'denim':.46,'hair':.18,'rib':.07}.get(kind,.06)
    ramp.color_ramp.elements[0].position=.2
    ramp.color_ramp.elements[0].color=(*[c*(1-strength) for c in color],1)
    ramp.color_ramp.elements[1].position=.8
    ramp.color_ramp.elements[1].color=(*[min(1,c*(1+strength*.4)) for c in color],1)
    links.new(noise.outputs['Fac'],ramp.inputs[0])
    output=ramp.outputs['Color']
    if kind in ('fleece','denim','rib'):
        fiber=n.new('ShaderNodeTexNoise'); fiber.inputs['Scale'].default_value=100 if kind!='denim' else 110
        fiber.inputs['Detail'].default_value=2; fiber.inputs['Roughness'].default_value=.65
        links.new(tex.outputs['Object'],fiber.inputs['Vector'])
        micro=n.new('ShaderNodeValToRGB')
        amplitude=.14 if kind=='denim' else .045
        micro.color_ramp.elements[0].color=(1-amplitude,)*3+(1,)
        micro.color_ramp.elements[1].color=(1,1,1,1)
        links.new(fiber.outputs['Fac'],micro.inputs[0])
        mix=n.new('ShaderNodeMixRGB'); mix.blend_type='MULTIPLY'; mix.inputs[0].default_value=1
        links.new(output,mix.inputs[1]); links.new(micro.outputs['Color'],mix.inputs[2]); output=mix.outputs[0]
    if kind in ('denim','rib'):
        wave=n.new('ShaderNodeTexWave'); wave.wave_type='BANDS'; wave.bands_direction='DIAGONAL' if kind=='denim' else 'X'
        wave.inputs['Scale'].default_value=55 if kind=='denim' else 75
        wave.inputs['Distortion'].default_value=.4
        links.new(tex.outputs['Object'],wave.inputs['Vector'])
        mix=n.new('ShaderNodeMixRGB'); mix.blend_type='MULTIPLY'; mix.inputs[0].default_value=.12 if kind=='denim' else .08
        links.new(output,mix.inputs[1]); links.new(wave.outputs['Color'],mix.inputs[2]); output=mix.outputs[0]
    ao=n.new('ShaderNodeAmbientOcclusion'); ao.inputs['Distance'].default_value=.16
    ao.samples=24
    shade=n.new('ShaderNodeMixRGB'); shade.blend_type='MULTIPLY'; shade.inputs[0].default_value=.76
    links.new(output,shade.inputs[1]); links.new(ao.outputs['AO'],shade.inputs[2])
    links.new(shade.outputs[0],emit.inputs['Color'])
    return mat


def materials():
    # Neutral cloth bakes multiply the live primary color in the engine.
    return {
        'ink':material('Graphic face ink',(.0025,.0015,.001)),
        'skin':material('Warm skin',(.92,.89,.84),tint=True,display=(.64,.39,.22)),
        'fleece':material('Brushed purple fleece',(.87,.87,.87),'fleece',True,(.46,.28,.66)),
        'rib':material('Dense rib knit',(.74,.74,.74),'rib',True,(.39,.23,.57)),
        'seam':material('Deep purple seam',(.16,.075,.235)),
        'stitch':material('Purple topstitch',(.35,.19,.50)),
        'cotton':material('Ivory woven cotton',(.88,.865,.82),'fleece'),
        'denim':material('Indigo crosswoven denim',(.030,.047,.069),'denim'),
        'denim-hem':material('Turned indigo denim',(.044,.066,.090),'denim'),
        'thread':material('Denim topstitch',(.16,.15,.12)),
        'rubber':material('Warm white cupsole',(.76,.755,.725)),
        'leather':material('Soft white leather',(.88,.86,.815)),
        'sock':material('White ribbed sock',(.90,.88,.85),'rib'),
        'shoe-thread':material('Sole grooves and stitches',(.54,.54,.515)),
        'hair':material('Chestnut swept hair',(.072,.023,.009),'hair'),
    }
